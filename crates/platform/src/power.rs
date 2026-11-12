//! Resume and power source events from Windows, through power callbacks (no
//! window needed).

use std::ffi::c_void;
use std::io;
use std::ptr;
use std::sync::mpsc::{self, Receiver, Sender};

use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Power::{
    DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS, GetSystemPowerStatus, HPOWERNOTIFY, POWERBROADCAST_SETTING,
    PowerRegisterSuspendResumeNotification, PowerSettingRegisterNotification,
    PowerSettingUnregisterNotification, PowerUnregisterSuspendResumeNotification,
    SYSTEM_POWER_STATUS,
};
use windows_sys::Win32::System::SystemServices::GUID_ACDC_POWER_SOURCE;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DEVICE_NOTIFY_CALLBACK, GetSystemMetrics, PBT_APMRESUMEAUTOMATIC, PBT_POWERSETTINGCHANGE,
    SM_SHUTTINGDOWN,
};

use windows_sys::core::GUID;

use crate::{PowerEvent, PowerEvents, windows_error};

/// The AC line status while the laptop runs on its battery.
const AC_OFFLINE: u8 = 0;
/// The AC line status while the charger is plugged in.
const AC_ONLINE: u8 = 1;

/// Whether the laptop runs on its battery now, or `None` if Windows does
/// not know or the read fails. The caller picks the safe side.
#[must_use]
pub fn on_battery() -> Option<bool> {
    let mut status = SYSTEM_POWER_STATUS::default();
    // SAFETY: Windows writes only the status.
    if unsafe { GetSystemPowerStatus(&raw mut status) } == 0 {
        return None;
    }
    on_battery_for(status.ACLineStatus)
}

/// Whether an AC line status means battery. Unknown (255) and any other
/// value give `None`.
fn on_battery_for(ac_line_status: u8) -> Option<bool> {
    match ac_line_status {
        AC_OFFLINE => Some(true),
        AC_ONLINE => Some(false),
        _ => None,
    }
}

/// Power events from Windows, from [`register`](Self::register) until drop.
///
/// Only the automatic resume is passed on. Windows sends it on every resume,
/// with or without a user at the keyboard, so each resume is one event.
/// The power source comes once at register, then on each change.
pub struct WindowsPowerEvents {
    events: Receiver<PowerEvent>,
    registration: HPOWERNOTIFY,
    power_source: HPOWERNOTIFY,
    // Windows calls back with pointers into these, so they live until drop.
    _params: Box<DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS>,
    _sender: Box<Sender<PowerEvent>>,
}

// SAFETY: the params only hold pointers to the callback and to the boxed
// sender, which is safe to use from any thread. The registration handle is
// a plain value that any thread can unregister.
unsafe impl Send for WindowsPowerEvents {}

impl WindowsPowerEvents {
    /// Asks Windows to tell us about each resume and power source change.
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
        let registration = registration as HPOWERNOTIFY;
        let mut power_source = ptr::null_mut();
        // SAFETY: as above; the same params serve both registrations.
        let status = unsafe {
            PowerSettingRegisterNotification(
                &GUID_ACDC_POWER_SOURCE,
                DEVICE_NOTIFY_CALLBACK,
                ptr::from_mut(&mut *params).cast(),
                &raw mut power_source,
            )
        };
        if status != ERROR_SUCCESS {
            // SAFETY: the first registration succeeded and is not kept.
            unsafe { PowerUnregisterSuspendResumeNotification(registration) };
            return Err(windows_error(status));
        }
        Ok(Self {
            events,
            registration,
            power_source: power_source as HPOWERNOTIFY,
            _params: params,
            _sender: sender,
        })
    }
}

impl PowerEvents for WindowsPowerEvents {
    /// Waits for the next event. It never gives `None`: it waits until the
    /// app ends.
    fn next_event(&mut self) -> Option<PowerEvent> {
        self.events.recv().ok()
    }
}

impl Drop for WindowsPowerEvents {
    fn drop(&mut self) {
        // SAFETY: the handles came from successful registrations and are
        // unregistered only here, once. The errors are ignored: there is
        // nothing left to do with them.
        unsafe {
            PowerSettingUnregisterNotification(self.power_source);
            PowerUnregisterSuspendResumeNotification(self.registration);
        }
    }
}

/// Whether Windows is shutting down, restarting or signing the user out.
#[must_use]
pub fn shutting_down() -> bool {
    // SAFETY: a plain query with no pointers.
    unsafe { GetSystemMetrics(SM_SHUTTINGDOWN) != 0 }
}

/// Called by Windows on its own thread for each power event.
unsafe extern "system" fn on_power_event(
    context: *const c_void,
    kind: u32,
    setting: *const c_void,
) -> u32 {
    let event = match kind {
        PBT_APMRESUMEAUTOMATIC => Some(PowerEvent::Resumed),
        // SAFETY: for this kind Windows passes a setting change.
        PBT_POWERSETTINGCHANGE => unsafe { power_source(setting.cast()) },
        _ => None,
    };
    if let Some(event) = event {
        // SAFETY: the context is the boxed sender, kept until unregistered.
        let sender = unsafe { &*context.cast::<Sender<PowerEvent>>() };
        // An error only means nobody is listening any more.
        let _ = sender.send(event);
    }
    ERROR_SUCCESS
}

/// The power source a setting change reports, or `None` for any other
/// setting. Any source but AC counts as the battery.
///
/// # Safety
///
/// `setting` points to a setting change whose data is `DataLength` bytes.
unsafe fn power_source(setting: *const POWERBROADCAST_SETTING) -> Option<PowerEvent> {
    // SAFETY: the caller passes a whole setting change.
    let header = unsafe { &*setting };
    if !is_power_source(&header.PowerSetting) || header.DataLength < 4 {
        return None;
    }
    // SAFETY: the data holds at least the 4 bytes of a DWORD, maybe unaligned.
    let value = unsafe { ptr::read_unaligned((&raw const header.Data).cast::<u32>()) };
    Some(if value == 0 {
        PowerEvent::OnAc
    } else {
        PowerEvent::OnBattery
    })
}

/// Whether a setting is the AC/DC power source. `GUID` has no `==`.
fn is_power_source(setting: &GUID) -> bool {
    let wanted = GUID_ACDC_POWER_SOURCE;
    (setting.data1, setting.data2, setting.data3, setting.data4)
        == (wanted.data1, wanted.data2, wanted.data3, wanted.data4)
}

#[cfg(test)]
mod tests {
    use windows_sys::Win32::System::SystemServices::GUID_ACDC_POWER_SOURCE;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        PBT_APMRESUMESUSPEND, PBT_APMSUSPEND, PBT_POWERSETTINGCHANGE,
    };

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
    fn the_ac_line_status_gives_the_power_source() {
        assert_eq!(on_battery_for(0), Some(true));
        assert_eq!(on_battery_for(1), Some(false));
        assert_eq!(on_battery_for(255), None);
        assert_eq!(on_battery_for(2), None);
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
