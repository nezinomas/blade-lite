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

use crate::{PowerEvent, PowerEvents};

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

fn windows_error(status: u32) -> io::Error {
    io::Error::from_raw_os_error(i32::try_from(status).unwrap_or(i32::MAX))
}

#[cfg(test)]
mod tests {
    use windows_sys::Win32::UI::WindowsAndMessaging::{PBT_APMRESUMESUSPEND, PBT_APMSUSPEND};

    use super::*;

    fn call(sender: &Sender<PowerEvent>, kind: u32) {
        let context = ptr::from_ref(sender).cast();
        // SAFETY: the context is a live sender, as Windows would pass it.
        unsafe { on_power_event(context, kind, ptr::null()) };
    }

    #[test]
    fn only_the_automatic_resume_is_passed_on() {
        let (sender, events) = mpsc::channel();

        for kind in [PBT_APMSUSPEND, PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMESUSPEND] {
            call(&sender, kind);
        }

        assert_eq!(events.try_iter().collect::<Vec<_>>(), [PowerEvent::Resumed]);
    }
}
