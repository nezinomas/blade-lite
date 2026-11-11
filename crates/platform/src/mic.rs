//! The mute of the default mic (the default capture device in Windows
//! Sound), read and set through its endpoint volume. Needs no admin.

use std::io;

use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
use windows::Win32::Media::Audio::{IMMDeviceEnumerator, MMDeviceEnumerator, eCapture, eConsole};
use windows::Win32::System::Com::{
    CLSCTX_ALL, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance, CoInitializeEx,
    CoUninitialize,
};

/// Whether the default mic is muted now.
///
/// # Errors
///
/// Any COM error, such as when there is no mic.
pub fn mic_muted() -> io::Result<bool> {
    with_mic(|volume| {
        // SAFETY: `volume` is a live endpoint volume from `with_mic`.
        let muted = unsafe { volume.GetMute() }?;
        Ok(muted.as_bool())
    })
}

/// Mutes the default mic if it is not muted, or the other way round.
/// Returns whether it is muted after, read back.
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
            Ok(volume.GetMute()?.as_bool())
        }
    })
}

/// Runs `act` on the default mic's endpoint volume, with COM started on
/// this thread for the call.
fn with_mic<T>(
    act: impl FnOnce(&IAudioEndpointVolume) -> windows::core::Result<T>,
) -> io::Result<T> {
    // SAFETY: no pointer is passed. A thread that already has COM gets
    // S_FALSE or RPC_E_CHANGED_MODE, and the calls below still work.
    let com = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) };
    let result = endpoint_volume().and_then(|volume| act(&volume));
    if com.is_ok() {
        // SAFETY: pairs the CoInitializeEx above that worked. The COM
        // objects of this call were dropped inside `result`'s making.
        unsafe { CoUninitialize() };
    }
    result.map_err(|error| io::Error::other(format!("the mic mute: {error}")))
}

/// The default mic's endpoint volume.
fn endpoint_volume() -> windows::core::Result<IAudioEndpointVolume> {
    // SAFETY: plain COM calls with the class and interface ids of the
    // `windows` crate; each object is released when dropped.
    unsafe {
        let devices: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let mic = devices.GetDefaultAudioEndpoint(eCapture, eConsole)?;
        mic.Activate(CLSCTX_ALL, None)
    }
}
