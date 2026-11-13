//! The mute of the default mic (the default capture device in Windows
//! Sound), read and set through its endpoint volume. Needs no admin.
//!
//! [`MicWatch`] keeps the mute as Windows' change notices tell it, so a
//! look asks Windows nothing (ticket 188).

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use windows::Win32::Foundation::PROPERTYKEY;
use windows::Win32::Media::Audio::Endpoints::{
    IAudioEndpointVolume, IAudioEndpointVolumeCallback, IAudioEndpointVolumeCallback_Impl,
};
use windows::Win32::Media::Audio::{
    AUDIO_VOLUME_NOTIFICATION_DATA, DEVICE_STATE, EDataFlow, ERole, IMMDeviceEnumerator,
    IMMNotificationClient, IMMNotificationClient_Impl, MMDeviceEnumerator, eCapture, eConsole,
};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance, CoInitializeEx,
    CoUninitialize,
};
use windows::core::{PCWSTR, implement};

/// Mutes the default mic if it is not muted, or the other way round.
/// Returns whether it is muted after, read back, or the state set if the
/// read back fails.
///
/// # Errors
///
/// Any COM error, such as when there is no mic. Nothing is set if the
/// mute can't be read.
pub fn toggle_mic_mute() -> io::Result<bool> {
    with_mic(|volume| {
        // SAFETY: `volume` is a live endpoint volume from `with_mic`, and
        // a null event context is allowed.
        unsafe {
            let muted = volume.GetMute()?.as_bool();
            volume.SetMute(!muted, std::ptr::null())?;
            Ok(volume
                .GetMute()
                .map_or(!muted, windows::core::BOOL::as_bool))
        }
    })
}

/// Runs `act` on the default mic's endpoint volume, with COM started on
/// this thread for the call.
fn with_mic<T>(
    act: impl FnOnce(&IAudioEndpointVolume) -> windows::core::Result<T>,
) -> io::Result<T> {
    let _com = ComStarted::new();
    // The COM objects of this call are dropped inside, before COM ends.
    endpoint_volume()
        .and_then(|volume| act(&volume))
        .map_err(|error| mic_error(&error))
}

/// The default mic's endpoint volume.
fn endpoint_volume() -> windows::core::Result<IAudioEndpointVolume> {
    // SAFETY: a plain COM call with the class id of the `windows` crate;
    // the object is released when dropped.
    let devices: IMMDeviceEnumerator =
        unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)? };
    endpoint_volume_from(&devices)
}

/// The default mic's endpoint volume, from `devices`.
fn endpoint_volume_from(
    devices: &IMMDeviceEnumerator,
) -> windows::core::Result<IAudioEndpointVolume> {
    // SAFETY: plain COM calls on a live enumerator; each object is
    // released when dropped.
    unsafe {
        let mic = devices.GetDefaultAudioEndpoint(eCapture, eConsole)?;
        mic.Activate(CLSCTX_ALL, None)
    }
}

/// What the notices left for the next look. Shared with Windows' notice
/// threads, so atomics only.
#[derive(Debug)]
struct MicState {
    /// [`UNKNOWN`], [`UNMUTED`] or [`MUTED`].
    muted: AtomicU8,
    /// No read was made yet, or the default mic may have changed: the next
    /// look reads.
    stale: AtomicBool,
}

const UNKNOWN: u8 = 0;
const UNMUTED: u8 = 1;
const MUTED: u8 = 2;

impl Default for MicState {
    /// Unknown and stale, so the first look reads.
    fn default() -> Self {
        Self {
            muted: AtomicU8::new(UNKNOWN),
            stale: AtomicBool::new(true),
        }
    }
}

impl MicState {
    /// Notes the mute a read or a notice gave (`None`: it can't be read).
    fn note_mute(&self, muted: Option<bool>) {
        let value = match muted {
            None => UNKNOWN,
            Some(false) => UNMUTED,
            Some(true) => MUTED,
        };
        self.muted.store(value, Ordering::Release);
    }

    /// Notes the mute a read gave, only if no notice has noted one since
    /// the mute was made unknown: a notice is newer than a read it raced.
    fn note_read(&self, muted: bool) {
        let value = if muted { MUTED } else { UNMUTED };
        let _ = self
            .muted
            .compare_exchange(UNKNOWN, value, Ordering::AcqRel, Ordering::Acquire);
    }

    /// The mute as last noted; `None` if unknown.
    fn muted(&self) -> Option<bool> {
        match self.muted.load(Ordering::Acquire) {
            UNMUTED => Some(false),
            MUTED => Some(true),
            _ => None,
        }
    }

    /// Makes the next look read.
    fn read_again(&self) {
        self.stale.store(true, Ordering::Release);
    }

    /// Whether a read is due, and clears it.
    fn take_stale(&self) -> bool {
        self.stale.swap(false, Ordering::AcqRel)
    }
}

/// Whether a default-device notice is for the default mic, the one the
/// app reads (capture, console role).
fn is_default_mic(flow: EDataFlow, role: ERole) -> bool {
    flow == eCapture && role == eConsole
}

/// The default mic's mute, kept from Windows' notices: the endpoint
/// volume's change notice for the mute, and the device notice for a new
/// default mic, after which the next look reads. Lives on the thread that
/// made it (it keeps COM started there); the notices come on Windows' own
/// threads and touch only [`MicState`].
pub struct MicWatch {
    state: Arc<MicState>,
    /// The default mic's volume and its registered notice, once read.
    mic: Option<(IAudioEndpointVolume, IAudioEndpointVolumeCallback)>,
    client: IMMNotificationClient,
    devices: IMMDeviceEnumerator,
    /// Last, so COM ends after the objects above are released.
    _com: ComStarted,
}

impl MicWatch {
    /// Starts COM on this thread for the watch's life and registers for
    /// default-device notices. The first look reads the mute.
    ///
    /// # Errors
    ///
    /// Any COM error.
    pub fn new() -> io::Result<Self> {
        let com = ComStarted::new();
        let state = Arc::new(MicState::default());
        let client: IMMNotificationClient = DeviceNotices(Arc::clone(&state)).into();
        // SAFETY: plain COM calls with the class id of the `windows` crate;
        // the client is unregistered in `drop`.
        let devices = unsafe {
            let devices: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                    .map_err(|error| mic_error(&error))?;
            devices
                .RegisterEndpointNotificationCallback(&client)
                .map_err(|error| mic_error(&error))?;
            devices
        };
        Ok(Self {
            state,
            mic: None,
            client,
            devices,
            _com: com,
        })
    }

    /// The mute as the notices keep it, or read again (from the default
    /// mic, whose change notice is then registered) at the first look,
    /// after a new default mic, after [`read_again`](Self::read_again), or
    /// after a failed read. `None` if unknown.
    ///
    /// # Errors
    ///
    /// A read that failed; the next look tries again.
    pub fn muted(&mut self) -> io::Result<Option<bool>> {
        if self.state.take_stale()
            && let Err(error) = self.watch_default_mic()
        {
            self.state.note_mute(None);
            self.state.read_again();
            return Err(mic_error(&error));
        }
        Ok(self.state.muted())
    }

    /// Makes the next look read, as after a wake.
    pub fn read_again(&self) {
        self.state.read_again();
    }

    /// Drops the old mic's notice, then takes the default mic's volume,
    /// registers its change notice and reads its mute.
    fn watch_default_mic(&mut self) -> windows::core::Result<()> {
        self.unwatch_mic();
        self.state.note_mute(None);
        let volume = endpoint_volume_from(&self.devices)?;
        let notice: IAudioEndpointVolumeCallback = VolumeNotices(Arc::clone(&self.state)).into();
        // SAFETY: `volume` is a live endpoint volume; the notice is
        // unregistered in `unwatch_mic`.
        unsafe { volume.RegisterControlChangeNotify(&notice) }?;
        self.mic = Some((volume.clone(), notice));
        // SAFETY: as above. Read after the register, so no change is lost;
        // a notice that came meanwhile is newer and is kept.
        let muted = unsafe { volume.GetMute() }?;
        self.state.note_read(muted.as_bool());
        Ok(())
    }

    fn unwatch_mic(&mut self) {
        if let Some((volume, notice)) = self.mic.take() {
            // SAFETY: the notice was registered on this volume. A failure
            // (the mic is gone) leaves nothing to undo.
            let _ = unsafe { volume.UnregisterControlChangeNotify(&notice) };
        }
    }
}

impl Drop for MicWatch {
    fn drop(&mut self) {
        self.unwatch_mic();
        // SAFETY: the client was registered in `new`.
        let _ = unsafe {
            self.devices
                .UnregisterEndpointNotificationCallback(&self.client)
        };
    }
}

/// COM started on this thread, ended when dropped if the start worked.
struct ComStarted(bool);

impl ComStarted {
    fn new() -> Self {
        // SAFETY: no pointer is passed. A thread that already has COM gets
        // S_FALSE (paired in `drop`) or RPC_E_CHANGED_MODE (not paired),
        // and the calls still work.
        let started =
            unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
        Self(started.is_ok())
    }
}

impl Drop for ComStarted {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: pairs the CoInitializeEx in `new` that worked.
            unsafe { CoUninitialize() };
        }
    }
}

/// Windows' notices, as COM objects.
mod notices {
    #![allow(
        clippy::ref_as_ptr,
        clippy::inline_always,
        reason = "in the code the windows crate's #[implement] makes"
    )]

    use super::{
        AUDIO_VOLUME_NOTIFICATION_DATA, Arc, DEVICE_STATE, EDataFlow, ERole,
        IAudioEndpointVolumeCallback, IAudioEndpointVolumeCallback_Impl, IMMNotificationClient,
        IMMNotificationClient_Impl, MicState, PCWSTR, PROPERTYKEY, implement, is_default_mic,
    };

    /// The endpoint volume's change notice: keeps the mute.
    #[implement(IAudioEndpointVolumeCallback)]
    pub(super) struct VolumeNotices(pub(super) Arc<MicState>);

    impl IAudioEndpointVolumeCallback_Impl for VolumeNotices_Impl {
        fn OnNotify(&self, data: *mut AUDIO_VOLUME_NOTIFICATION_DATA) -> windows::core::Result<()> {
            // SAFETY: Windows passes data valid for the call, or null.
            if let Some(data) = unsafe { data.as_ref() } {
                self.0.note_mute(Some(data.bMuted.as_bool()));
            }
            Ok(())
        }
    }

    /// The device notices: a new default mic makes the next look read.
    #[implement(IMMNotificationClient)]
    pub(super) struct DeviceNotices(pub(super) Arc<MicState>);

    impl IMMNotificationClient_Impl for DeviceNotices_Impl {
        fn OnDefaultDeviceChanged(
            &self,
            flow: EDataFlow,
            role: ERole,
            _id: &PCWSTR,
        ) -> windows::core::Result<()> {
            if is_default_mic(flow, role) {
                self.0.read_again();
            }
            Ok(())
        }

        fn OnDeviceStateChanged(
            &self,
            _id: &PCWSTR,
            _state: DEVICE_STATE,
        ) -> windows::core::Result<()> {
            Ok(())
        }

        fn OnDeviceAdded(&self, _id: &PCWSTR) -> windows::core::Result<()> {
            Ok(())
        }

        fn OnDeviceRemoved(&self, _id: &PCWSTR) -> windows::core::Result<()> {
            Ok(())
        }

        fn OnPropertyValueChanged(
            &self,
            _id: &PCWSTR,
            _key: &PROPERTYKEY,
        ) -> windows::core::Result<()> {
            Ok(())
        }
    }
}

use notices::{DeviceNotices, VolumeNotices};

fn mic_error(error: &windows::core::Error) -> io::Error {
    io::Error::other(format!("the mic mute: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Media::Audio::{eCommunications, eRender};

    #[test]
    fn the_first_look_reads() {
        let state = MicState::default();

        assert!(state.take_stale());
        assert!(!state.take_stale());
        assert_eq!(state.muted(), None);
    }

    #[test]
    fn a_noted_mute_is_kept() {
        let state = MicState::default();

        state.note_mute(Some(true));
        assert_eq!(state.muted(), Some(true));
        state.note_mute(Some(false));
        assert_eq!(state.muted(), Some(false));
        state.note_mute(None);
        assert_eq!(state.muted(), None);
    }

    #[test]
    fn a_read_does_not_overwrite_a_notice_that_came_meanwhile() {
        let state = MicState::default();
        state.note_mute(None);

        state.note_mute(Some(true));
        state.note_read(false);

        assert_eq!(state.muted(), Some(true));
    }

    #[test]
    fn a_read_with_no_notice_meanwhile_is_kept() {
        let state = MicState::default();
        state.note_mute(None);

        state.note_read(true);

        assert_eq!(state.muted(), Some(true));
    }

    #[test]
    fn read_again_makes_one_read_due() {
        let state = MicState::default();
        state.take_stale();

        state.read_again();

        assert!(state.take_stale());
        assert!(!state.take_stale());
    }

    #[test]
    fn only_the_default_console_mic_counts() {
        assert!(is_default_mic(eCapture, eConsole));
        assert!(!is_default_mic(eRender, eConsole));
        assert!(!is_default_mic(eCapture, eCommunications));
    }
}
