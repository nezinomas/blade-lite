//! Resume events from Windows, through a power callback (no window needed).

use std::ffi::c_void;
use std::io;
use std::ptr;
use std::sync::mpsc::{self, Receiver, Sender};

use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Power::{
    DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS, HPOWERNOTIFY, PowerRegisterSuspendResumeNotification,
    PowerUnregisterSuspendResumeNotification,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{DEVICE_NOTIFY_CALLBACK, PBT_APMRESUMEAUTOMATIC};

use crate::{PowerEvent, PowerEvents, windows_error};

/// Resume events from Windows, from [`register`](Self::register) until drop.
///
/// Only the automatic resume is passed on. Windows sends it on every resume,
/// with or without a user at the keyboard, so each resume is one event.
pub struct WindowsPowerEvents {
    events: Receiver<PowerEvent>,
    registration: HPOWERNOTIFY,
    // Windows calls back with pointers into these, so they live until drop.
    _params: Box<DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS>,
    _sender: Box<Sender<PowerEvent>>,
}

// SAFETY: the params only hold pointers to the callback and to the boxed
// sender, which is safe to use from any thread. The registration handle is
// a plain value that any thread can unregister.
unsafe impl Send for WindowsPowerEvents {}

impl WindowsPowerEvents {
    /// Asks Windows to tell us about each resume.
    ///
    /// # Errors
    ///
    /// The Windows error if the registration fails.
    pub fn register() -> io::Result<Self> {
        let (sender, events) = mpsc::channel();
        let sender = Box::new(sender);
        let mut params = Box::new(DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS {
            Callback: Some(on_power_event),
            Context: ptr::from_ref::<Sender<PowerEvent>>(&sender)
                .cast_mut()
                .cast(),
        });
        let mut registration = ptr::null_mut();
        // SAFETY: the params and the sender they point to are boxed, so they
        // stay put, and they are kept until drop unregisters the callback.
        let status = unsafe {
            PowerRegisterSuspendResumeNotification(
                DEVICE_NOTIFY_CALLBACK,
                ptr::from_mut(&mut *params).cast(),
                &raw mut registration,
            )
        };
        if status != ERROR_SUCCESS {
            return Err(windows_error(status));
        }
        Ok(Self {
            events,
            registration: registration as HPOWERNOTIFY,
            _params: params,
            _sender: sender,
        })
    }
}

impl PowerEvents for WindowsPowerEvents {
    /// Waits for the next resume. It never gives `None`: it waits until the
    /// app ends.
    fn next_event(&mut self) -> Option<PowerEvent> {
        self.events.recv().ok()
    }
}

impl Drop for WindowsPowerEvents {
    fn drop(&mut self) {
        // SAFETY: the handle came from a successful registration and is
        // unregistered only here, once. The error is ignored: there is
        // nothing left to do with it.
        unsafe { PowerUnregisterSuspendResumeNotification(self.registration) };
    }
}

/// Called by Windows on its own thread for each power event.
unsafe extern "system" fn on_power_event(
    context: *const c_void,
    kind: u32,
    _setting: *const c_void,
) -> u32 {
    if kind == PBT_APMRESUMEAUTOMATIC {
        // SAFETY: the context is the boxed sender, kept until unregistered.
        let sender = unsafe { &*context.cast::<Sender<PowerEvent>>() };
        // An error only means nobody is listening any more.
        let _ = sender.send(PowerEvent::Resumed);
    }
    ERROR_SUCCESS
}

#[cfg(test)]
mod tests {
    use windows_sys::Win32::System::SystemServices::GUID_ACDC_POWER_SOURCE;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        PBT_APMRESUMESUSPEND, PBT_APMSUSPEND, PBT_POWERSETTINGCHANGE,
    };
    use windows_sys::core::GUID;

    use super::*;

    /// A power setting change as Windows lays it out: a DWORD after the header.
    #[repr(C)]
    struct SettingChange {
        setting: GUID,
        length: u32,
        value: u32,
    }

    fn power_source(value: u32) -> SettingChange {
        SettingChange {
            setting: GUID_ACDC_POWER_SOURCE,
            length: 4,
            value,
        }
    }

    fn call(sender: &Sender<PowerEvent>, kind: u32, setting: Option<&SettingChange>) {
        let context = ptr::from_ref(sender).cast();
        let setting = setting.map_or(ptr::null(), |setting| ptr::from_ref(setting).cast());
        // SAFETY: the context is a live sender and the setting a whole
        // change, as Windows would pass them.
        unsafe { on_power_event(context, kind, setting) };
    }

    #[test]
    fn only_the_automatic_resume_is_passed_on() {
        let (sender, events) = mpsc::channel();

        for kind in [PBT_APMSUSPEND, PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMESUSPEND] {
            call(&sender, kind, None);
        }

        assert_eq!(events.try_iter().collect::<Vec<_>>(), [PowerEvent::Resumed]);
    }

    #[test]
    fn the_power_source_is_passed_on() {
        let (sender, events) = mpsc::channel();

        // 0 is AC, 1 is the battery, 2 is a short-term source such as a UPS.
        for value in [1, 0, 2] {
            call(&sender, PBT_POWERSETTINGCHANGE, Some(&power_source(value)));
        }

        assert_eq!(
            events.try_iter().collect::<Vec<_>>(),
            [
                PowerEvent::OnBattery,
                PowerEvent::OnAc,
                PowerEvent::OnBattery
            ]
        );
    }

    #[test]
    fn other_settings_are_ignored() {
        let (sender, events) = mpsc::channel();
        let other = SettingChange {
            setting: GUID::from_u128(1),
            ..power_source(1)
        };

        call(&sender, PBT_POWERSETTINGCHANGE, Some(&other));

        assert_eq!(events.try_iter().count(), 0);
    }
}
