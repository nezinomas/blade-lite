//! Sleep, resume and power source events from Windows, through power
//! callbacks (no window needed).

use std::ffi::c_void;
use std::io;
use std::ptr;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Power::{
    DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS, GetSystemPowerStatus, HPOWERNOTIFY, POWERBROADCAST_SETTING,
    PowerRegisterSuspendResumeNotification, PowerSettingRegisterNotification,
    PowerSettingUnregisterNotification, PowerUnregisterSuspendResumeNotification,
    SYSTEM_POWER_STATUS,
};
use windows_sys::Win32::System::SystemServices::GUID_ACDC_POWER_SOURCE;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DEVICE_NOTIFY_CALLBACK, GetSystemMetrics, PBT_APMRESUMEAUTOMATIC, PBT_APMSUSPEND,
    PBT_POWERSETTINGCHANGE, SM_SHUTTINGDOWN,
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

/// How long Windows' call that tells of a sleep is held for the app to send
/// (ticket 169). Windows gives the apps about 2 s for it, one time for all
/// of them (Microsoft's `PBT_APMSUSPEND` and Desktop Activity Moderator
/// pages), so a stuck app holds the sleep no longer than this.
pub const SUSPEND_WAIT: Duration = Duration::from_secs(1);

/// The sleep notice last passed on, and whether Windows' call still waits
/// for it.
#[derive(Clone, Copy, Default)]
struct Notice {
    /// Counts the sleep notices, so an answer for an old one is ignored.
    number: u64,
    waiting: bool,
}

/// Tells the call that told of a sleep that the app has acted on it, so the
/// call can return and Windows can go on; and tells the app whether that
/// call still waits, as only then is the app sure not to have been frozen
/// and thawed since.
#[derive(Clone, Default)]
pub struct SuspendHandled(Arc<(Mutex<Notice>, Condvar)>);

impl SuspendHandled {
    fn notice(&self) -> std::sync::MutexGuard<'_, Notice> {
        self.0.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The number of the last sleep notice, for [`done`](Self::done).
    #[must_use]
    pub fn number(&self) -> u64 {
        self.notice().number
    }

    /// Whether Windows' call for the last sleep notice still waits: the app
    /// has not been frozen since it came.
    #[must_use]
    pub fn waiting(&self) -> bool {
        self.notice().waiting
    }

    /// The app has acted on the notice `number`, or given up: its call
    /// returns. An answer for an older notice changes nothing.
    pub fn done(&self, number: u64) {
        let mut notice = self.notice();
        if notice.number == number {
            notice.waiting = false;
            self.0.1.notify_all();
        }
    }

    /// Starts a new notice, before it is passed on.
    fn arm(&self) {
        let mut notice = self.notice();
        notice.number = notice.number.wrapping_add(1);
        notice.waiting = true;
    }

    /// Waits until [`done`](Self::done), at most `limit`; after that the
    /// notice no longer waits, however the wait ended.
    fn wait(&self, limit: Duration) {
        let guard = self.notice();
        // Answered, timed out or poisoned: each just returns, so Windows
        // goes on.
        let mut notice = match self
            .0
            .1
            .wait_timeout_while(guard, limit, |notice| notice.waiting)
        {
            Ok((notice, _)) => notice,
            Err(poisoned) => poisoned.into_inner().0,
        };
        notice.waiting = false;
    }
}

/// What Windows' calls reach: where the events go, and the sleep's answer.
struct Listener {
    sender: Sender<PowerEvent>,
    suspend: SuspendHandled,
}

/// Power events from Windows, from [`register`](Self::register) until drop.
///
/// Only the sleep and the automatic resume are passed on. Windows sends the
/// automatic resume on every resume, with or without a user at the
/// keyboard, so each resume is one event. On Modern Standby the sleep comes
/// as Windows is about to freeze the desktop apps, after the screen goes
/// off (Microsoft's Desktop Activity Moderator page); the call that tells
/// of it waits for [`SuspendHandled::done`], at most [`SUSPEND_WAIT`]. The
/// power source comes once at register, then on each change.
pub struct WindowsPowerEvents {
    events: Receiver<PowerEvent>,
    registration: HPOWERNOTIFY,
    power_source: HPOWERNOTIFY,
    // Windows calls back with pointers into these, so they live until drop.
    _params: Box<DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS>,
    listener: Box<Listener>,
}

// SAFETY: the params only hold pointers to the callback and to the boxed
// listener, whose sender and answer are safe to use from any thread. The
// registration handle is a plain value that any thread can unregister.
unsafe impl Send for WindowsPowerEvents {}

impl WindowsPowerEvents {
    /// Asks Windows to tell us about each resume and power source change.
    ///
    /// # Errors
    ///
    /// The Windows error if the registration fails.
    pub fn register() -> io::Result<Self> {
        let (sender, events) = mpsc::channel();
        let listener = Box::new(Listener {
            sender,
            suspend: SuspendHandled::default(),
        });
        let mut params = Box::new(DEVICE_NOTIFY_SUBSCRIBE_PARAMETERS {
            Callback: Some(on_power_event),
            Context: ptr::from_ref::<Listener>(&listener).cast_mut().cast(),
        });
        let mut registration = ptr::null_mut();
        // SAFETY: the params and the listener they point to are boxed, so they
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
            listener,
        })
    }

    /// What the app calls once it has acted on a sleep.
    #[must_use]
    pub fn suspend_handled(&self) -> SuspendHandled {
        self.listener.suspend.clone()
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

/// Called by Windows on its own thread for each power event. A sleep is
/// held until the app has acted on it, at most [`SUSPEND_WAIT`].
unsafe extern "system" fn on_power_event(
    context: *const c_void,
    kind: u32,
    setting: *const c_void,
) -> u32 {
    let event = match kind {
        PBT_APMSUSPEND => Some(PowerEvent::Suspending),
        PBT_APMRESUMEAUTOMATIC => Some(PowerEvent::Resumed),
        // SAFETY: for this kind Windows passes a setting change.
        PBT_POWERSETTINGCHANGE => unsafe { power_source(setting.cast()) },
        _ => None,
    };
    if let Some(event) = event {
        // SAFETY: the context is the boxed listener, kept until unregistered.
        let listener = unsafe { &*context.cast::<Listener>() };
        if event != PowerEvent::Suspending {
            // An error only means nobody is listening any more.
            let _ = listener.sender.send(event);
            return ERROR_SUCCESS;
        }
        // Its own handle, so the wait never touches the listener, which a
        // drop may free meanwhile.
        let suspend = listener.suspend.clone();
        suspend.arm();
        if listener.sender.send(event).is_ok() {
            suspend.wait(SUSPEND_WAIT);
        } else {
            // Nobody listens any more: no wait.
            suspend.done(suspend.number());
        }
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

    fn listener() -> (Listener, Receiver<PowerEvent>) {
        let (sender, events) = mpsc::channel();
        let suspend = SuspendHandled::default();
        (Listener { sender, suspend }, events)
    }

    fn call(listener: &Listener, kind: u32, setting: Option<&SettingChange>) {
        let context = ptr::from_ref(listener).cast();
        let setting = setting.map_or(ptr::null(), |setting| ptr::from_ref(setting).cast());
        // SAFETY: the context is a live listener and the setting a whole
        // change, as Windows would pass them.
        unsafe { on_power_event(context, kind, setting) };
    }

    /// Answers each sleep after `delay`, as the app does once it has sent,
    /// and gives back the events seen once the listener is gone.
    fn answer_after(
        listener: &Listener,
        events: Receiver<PowerEvent>,
        delay: Duration,
    ) -> std::thread::JoinHandle<Vec<PowerEvent>> {
        let handled = listener.suspend.clone();
        std::thread::spawn(move || {
            let mut seen = Vec::new();
            while let Ok(event) = events.recv() {
                if event == PowerEvent::Suspending {
                    assert!(handled.waiting());
                    std::thread::sleep(delay);
                    handled.done(handled.number());
                }
                seen.push(event);
            }
            seen
        })
    }

    #[test]
    fn the_ac_line_status_gives_the_power_source() {
        assert_eq!(on_battery_for(0), Some(true));
        assert_eq!(on_battery_for(1), Some(false));
        assert_eq!(on_battery_for(255), None);
        assert_eq!(on_battery_for(2), None);
    }

    #[test]
    fn only_the_sleep_and_the_automatic_resume_are_passed_on() {
        let (listener, events) = listener();
        let answers = answer_after(&listener, events, Duration::ZERO);

        for kind in [PBT_APMSUSPEND, PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMESUSPEND] {
            call(&listener, kind, None);
        }
        drop(listener);

        assert_eq!(
            answers.join().unwrap(),
            [PowerEvent::Suspending, PowerEvent::Resumed]
        );
    }

    #[test]
    fn a_sleep_is_held_until_the_app_has_acted() {
        let (listener, events) = listener();
        let delay = Duration::from_millis(100);
        let answers = answer_after(&listener, events, delay);
        let began = std::time::Instant::now();

        call(&listener, PBT_APMSUSPEND, None);

        let held = began.elapsed();
        assert!(held >= delay && held < SUSPEND_WAIT, "{held:?}");
        drop(listener);
        assert_eq!(answers.join().unwrap(), [PowerEvent::Suspending]);
    }

    #[test]
    fn a_sleep_nobody_answers_is_held_at_most_the_limit() {
        let (listener, _events) = listener();
        let began = std::time::Instant::now();

        call(&listener, PBT_APMSUSPEND, None);

        let held = began.elapsed();
        assert!(held >= SUSPEND_WAIT && held < SUSPEND_WAIT * 2, "{held:?}");
        assert!(!listener.suspend.waiting(), "the app may be frozen since");
    }

    #[test]
    fn an_answer_for_an_old_sleep_does_not_cut_a_new_one_short() {
        let (listener, events) = listener();
        let old = listener.suspend.number();
        let handled = listener.suspend.clone();
        let answers = std::thread::spawn(move || {
            while let Ok(_event) = events.recv() {
                handled.done(old);
            }
        });
        let began = std::time::Instant::now();

        call(&listener, PBT_APMSUSPEND, None);

        assert!(began.elapsed() >= SUSPEND_WAIT);
        drop(listener);
        answers.join().unwrap();
    }

    #[test]
    fn a_sleep_with_nobody_listening_is_not_held() {
        let (listener, events) = listener();
        drop(events);
        let began = std::time::Instant::now();

        call(&listener, PBT_APMSUSPEND, None);

        assert!(began.elapsed() < SUSPEND_WAIT);
    }

    #[test]
    fn the_power_source_is_passed_on() {
        let (listener, events) = listener();

        // 0 is AC, 1 is the battery, 2 is a short-term source such as a UPS.
        for value in [1, 0, 2] {
            call(
                &listener,
                PBT_POWERSETTINGCHANGE,
                Some(&power_source(value)),
            );
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
        let (listener, events) = listener();
        let other = SettingChange {
            setting: GUID::from_u128(1),
            ..power_source(1)
        };

        call(&listener, PBT_POWERSETTINGCHANGE, Some(&other));

        assert_eq!(events.try_iter().count(), 0);
    }
}
