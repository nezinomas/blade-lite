//! What the shell does on each poll: power events, Num Lock, the mic mute
//! and sensor reads.

use std::rc::Rc;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant, SystemTime};

use blade_core::{GUARD_HEAT_INTERVAL, HEAT_INTERVAL, RESUME_PAUSE, Readings};
use blade_lite::{AppError, report};
use platform::{PowerEvent, num_lock_on};
use razer_device::{CONTROLLER_CLOSED, CONTROLLER_OPENED_AGAIN};
use slint::Timer;
use ui::{DisplayView, FAN_GUARD_WAKE_HOLD, Header, HeatWatch};

use super::popup::{POPUP_HOLD, TOUCHPAD};
use super::{AppOn, Parts, Reopened, Shell};
use crate::bind;

/// The key under which a closed or held controller's read failure counts
/// in the quiet time.
const GONE_KEY: &str = "controller closed or held";

/// How long after a manual Custom send its fan targets go once more
/// (ticket 173; on the laptop 2 s worked).
const FAN_TARGETS_AGAIN: Duration = Duration::from_secs(2);

/// How long the app runs on after a sleep notice with no wake notice,
/// with input from the user in that time, before that counts as a wake
/// (ticket 170). Windows freezes the app a few seconds after the notice,
/// so a minute of ticks means it is likely not asleep; the input makes
/// sure.
const MISSED_WAKE_AFTER: Duration = Duration::from_secs(60);

/// The longest wall-clock gap between two ticks that still counts as one
/// run: a longer one may be a freeze in the sleep.
const TICK_GAP: Duration = Duration::from_secs(5);

/// How far into a run after a sleep notice input must come to count for a
/// missed wake: a touch while the screen goes dark comes before Windows
/// freezes the app. Longer than [`TICK_GAP`], so a small clock step can't
/// let input from before the notice in.
const INPUT_GRACE: Duration = Duration::from_secs(30);

/// A sleep at least this long by the wall clock lets the fan guard end its
/// full speed after a short cool hold (ticket 172): the chips cooled in it.
const LONG_SLEEP: Duration = Duration::from_mins(5);

#[cfg(test)]
thread_local! {
    /// The `sent again by a poll` lines this test's thread logged: the
    /// window tests share one log.
    pub static FULL_SPEED_RESENT_LINES: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

impl<P: Parts> Shell<P> {
    /// Passes on power events, tries a refused rate again, passes
    /// on Num Lock, then reads the sensors if a view shows them, or only
    /// the temperatures now and then if none does; neither while a resume
    /// waits. Each such read also reads the charger, and sends the preset
    /// again if a send failed before the last tick began and none failed
    /// since.
    pub fn tick(self: &Rc<Self>) {
        self.tick_at(Instant::now(), SystemTime::now());
    }

    /// [`tick`](Self::tick) at `now`, with the wall clock at `wall`. The
    /// window tests give their own times.
    pub(crate) fn tick_at(self: &Rc<Self>, now: Instant, wall: SystemTime) {
        let failures = self.app.borrow().preset_failures();
        let looked = self.look_for_razer_now_and_then(now);
        self.follow_own_power();
        self.follow_missed_wake(wall);
        self.follow_long_sleep(wall);
        self.start_after_updater();
        self.send_fan_targets_again(now);
        if looked {
            self.read_firmware_if_unread();
        }
        if self.resuming.get() == 0 && self.app.borrow().battery_rate_pending() {
            self.retry_battery_rate();
        }
        self.follow_num_lock(num_lock_on());
        // A failed read keeps the last state, so a flaky read can't
        // send a frame each poll.
        if let Some(muted) = P::mic_muted() {
            self.follow_mic(Some(muted));
        }
        // The controller may not be ready while a resume waits. A poll due
        // then stays due, so the first tick after the pause reads it.
        if self.resuming.get() == 0 && self.schedule.borrow_mut().poll_due(now) {
            self.tick_readings(now);
            self.after_reads();
        } else if self.heat_reads_allowed()
            && self
                .schedule
                .borrow_mut()
                .heat_due(now, wall, self.heat_interval())
        {
            self.tick_heat(now);
            self.after_reads();
        }
        self.preset_failures_seen.set(failures);
    }

    /// Sends the fan targets of a manual Custom once more, on the first
    /// tick at least [`FAN_TARGETS_AGAIN`] after the tick that saw it sent
    /// (ticket 173: sent from auto fans, the fans stall near 2900 RPM until
    /// the next target write). Not in a pause or a sleep, nor after the
    /// quit: it waits for the pause to end, and the wake's own send starts
    /// it again.
    fn send_fan_targets_again(self: &Rc<Self>, now: Instant) {
        let Some(due) = self.app.borrow().fan_targets_due() else {
            self.fan_targets_seen.set(None);
            return;
        };
        if self.resuming.get() > 0 || self.quit_sent.get() {
            return;
        }
        // Razer software may have set the fans on auto since (a target
        // write needs the manual flag), and an updater refuses every send:
        // the other background sends stop then too.
        if self.razer_warning.borrow().is_some() || self.app.borrow().controller_held() {
            self.app.borrow_mut().forget_fan_targets();
            self.fan_targets_seen.set(None);
            return;
        }
        let seen = match self.fan_targets_seen.get() {
            // A newer send waits its own 2 s.
            Some((seen_due, seen)) if seen_due == due => seen,
            _ => {
                self.fan_targets_seen.set(Some((due, now)));
                return;
            }
        };
        if now.saturating_duration_since(seen) >= FAN_TARGETS_AGAIN {
            self.fan_targets_seen.set(None);
            // Nothing shown changes, so the windows are filled again only
            // to show a failure (it is not tried again).
            let sent = self.app.borrow_mut().send_fan_targets_again();
            if let Err(error) = sent {
                self.refresh(Some(&error));
            }
        }
    }

    /// Follows the charger, or sends the preset again, unless a read just
    /// opened the controller again: the wake's pause then comes first.
    fn after_reads(self: &Rc<Self>) {
        if self.follow_reopen() {
            return;
        }
        if !self.follow_charger() {
            self.resend_preset();
        }
    }

    /// After the controller was opened again: clicks and polls wait out the
    /// wake's pause (the controller holds its new handle until then), then
    /// Razer software is looked for (a Razer driver install may be what
    /// killed the old handle), the handle is released and the settings are
    /// sent again, as the controller may have lost them. The power source is
    /// read again first, as power events in the pause are dropped. Unlike a
    /// wake, the heat watch is left alone, and the rate changes only with
    /// the source. Begun in a wake's pause, its end is the wake's, with the
    /// release and the firmware read (ticket 162). After an open that failed
    /// for a new reason, only the look. Returns whether a pause began.
    fn follow_reopen(self: &Rc<Self>) -> bool {
        match P::controller_reopened() {
            Reopened::No => false,
            Reopened::Failed => {
                if self.look_for_razer() {
                    self.show_window();
                }
                false
            }
            Reopened::Opened => {
                let pause = self.begin_pause();
                let shell = Rc::clone(self);
                Timer::single_shot(RESUME_PAUSE, move || {
                    shell.resuming.set(shell.resuming.get().saturating_sub(1));
                    // A sleep or a newer pause since: nothing reaches the
                    // controller, and the last pause does all this.
                    if shell.superseded(pause) {
                        shell.redo_at_wake.set(true);
                        return;
                    }
                    // Begun in a wake's pause: the wake's end, with this one's
                    // release and firmware read (ticket 162).
                    if shell.wake_due.get() {
                        shell.redo_at_wake.set(true);
                        shell.end_wake_pause();
                        return;
                    }
                    let first = shell.look_for_razer();
                    // This start covers an updater that just ended, and an
                    // older pause that left its work to this one.
                    shell.updater_ended.set(false);
                    shell.redo_at_wake.set(false);
                    P::release_controller();
                    shell.controller_gone.set(false);
                    // A firmware update may be what killed the old handle.
                    shell.app.borrow_mut().check_firmware();
                    // A power event in the pause was dropped: read the
                    // source, so a pulled charger sends the battery preset
                    // and rate before the settings go again.
                    shell.follow_source();
                    // As the wake: an unknown source runs the battery presets.
                    shell.app.borrow_mut().take_on_battery(P::on_battery());
                    shell.act(AppOn::<P>::start);
                    if first {
                        shell.show_window();
                    }
                });
                true
            }
        }
    }

    /// Sends the preset that runs again if its last send failed, so the
    /// laptop does not keep running another one (such as Performance on
    /// battery) until the next power event. Not while a resume waits or
    /// Razer's software runs, as for the heat reads, nor in the tick of a
    /// failed send or the one after it: the controller may still run it.
    /// A resend that works is quiet, so the error of the failed send stays
    /// shown (it may not be the preset's), unless a rate error was held
    /// behind that send: it is shown then, with the windows filled again.
    /// A failed one is shown. If the guard's own full-speed send failed,
    /// its log line said nothing went: a resend that takes it says so,
    /// even one that shows a held rate error (ticket 163).
    fn resend_preset(self: &Rc<Self>) {
        let quiet = self.app.borrow().preset_failures() == self.preset_failures_seen.get();
        if !quiet || !self.heat_reads_allowed() {
            return;
        }
        let unsent = self.app.borrow().boost_unsent();
        let sent = self.app.borrow_mut().resend_preset();
        let app = self.app.borrow();
        if unsent && !app.boost_unsent() && self.fan_guard.get().on() && app.manual_fan_runs() {
            report("fan guard: Full speed, sent again by a poll");
            #[cfg(test)]
            FULL_SPEED_RESENT_LINES.set(FULL_SPEED_RESENT_LINES.get() + 1);
        }
        drop(app);
        if let Err(error) = sent {
            self.act(|_| Err(error));
        }
    }

    /// Tells the fan guard whether the last GPU read failed (ticket 139).
    fn note_gpu_failed(&self, failed: bool) {
        let mut guard = self.fan_guard.get();
        guard.gpu_failed(failed);
        self.fan_guard.set(guard);
    }

    /// Sends the preset that runs if a weak charge came or went, such as a
    /// USB-C monitor plugged in while the Razer charger is pulled (Windows
    /// sees no power change then). Not while a resume waits or Razer's
    /// software runs, as for the heat reads. Returns whether it acted.
    fn follow_charger(self: &Rc<Self>) -> bool {
        if !self.heat_reads_allowed() {
            return false;
        }
        let changed = self.app.borrow_mut().follow_charger();
        let acted = !matches!(changed, Ok(false));
        if acted {
            self.act(|_| changed.map(|_| ()));
        }
        acted
    }

    /// The header, with the controller's readings only if
    /// [`heat_reads_allowed`](Self::heat_reads_allowed): while Razer's
    /// software runs its values show unknown.
    pub(super) fn header(&self, app: &mut AppOn<P>, readings: Readings) -> Header {
        if self.heat_reads_allowed() {
            app.header(readings)
        } else {
            app.header_unread(readings)
        }
    }

    /// The display view, with the native mode read only if
    /// [`heat_reads_allowed`](Self::heat_reads_allowed): else the one last
    /// read or set, or none chosen if none is known. A native mode read
    /// that names a mode the model lacks shows none chosen. A failed view
    /// is `None`. Each error is put in `errors`.
    pub(super) fn display_view(
        &self,
        app: &mut AppOn<P>,
        errors: &mut Vec<String>,
    ) -> Option<DisplayView> {
        let view = if self.heat_reads_allowed() {
            app.display_view()
        } else {
            app.display_view_unread()
        };
        let view = match view {
            Err(error @ AppError::UnknownNativeMode) => {
                errors.push(error.to_string());
                app.display_view_unread()
            }
            view => view,
        };
        view.map_err(|error| errors.push(error.to_string())).ok()
    }

    /// Whether the controller may be read in the background: not while a
    /// resume waits (it may not be ready), nor while Razer's own software
    /// runs (it may talk to the controller at the same time).
    pub(super) fn heat_reads_allowed(&self) -> bool {
        self.resuming.get() == 0 && self.razer_warning.borrow().is_none()
    }

    /// How often the temperatures are read with no view open: often while
    /// a manual Custom fan runs, as only the fan guard can then speed it up.
    fn heat_interval(&self) -> Duration {
        if self.app.borrow().manual_fan_runs() {
            GUARD_HEAT_INTERVAL
        } else {
            HEAT_INTERVAL
        }
    }

    /// Reads only the temperatures, for the heat warning.
    fn tick_heat(self: &Rc<Self>, now: Instant) {
        let (gpu, mut errors) = {
            let (sampler, sensors) = &mut *self.sampler.borrow_mut();
            let gpu = sampler.gpu_celsius(sensors);
            self.note_gpu_failed(sampler.gpu_failed());
            (gpu, sampler.errors().to_vec())
        };
        let cpu = {
            let mut app = self.app.borrow_mut();
            app.read_cpu_temperature();
            errors.extend_from_slice(app.read_errors());
            app.cpu_celsius()
        };
        self.say_read_errors(&errors);
        self.watch_heat(cpu, gpu, now);
    }

    /// Tries a wake's or a switch's refused 60 Hz, or a refused put-back at
    /// the first AC, again: Windows can refuse a set just after a wake, and
    /// a refused one would leave the panel at the wrong rate. Only the last
    /// failed try is shown, so a quiet try does not clear the other errors
    /// or read the controller. When the tries end, the window's rate buttons
    /// show the panel's rate (one display view, so one native mode read);
    /// the flyout is filled when it opens. A native mode the model lacks
    /// shows none chosen; its error is logged, and shown at the next fill.
    fn retry_battery_rate(self: &Rc<Self>) {
        let tried = self.app.borrow_mut().retry_battery_rate();
        if let Err(error) = tried {
            self.act(|_| Err(error));
            return;
        }
        let mut app = self.app.borrow_mut();
        if app.battery_rate_pending() {
            return;
        }
        let mut errors = Vec::new();
        if let Some(view) = self.display_view(&mut app, &mut errors) {
            bind::display(&self.window, &view);
        }
        self.say_read_errors(&errors);
    }

    /// Paints the touchpad key again if Num Lock changed since the last look.
    /// While a resume waits, the state is only kept: the resume paints it.
    pub fn follow_num_lock(self: &Rc<Self>, on: bool) {
        if self.app.borrow().num_lock_on() == on {
            return;
        }
        if self.resuming.get() > 0 {
            self.app.borrow_mut().set_num_lock(on);
            return;
        }
        // The windows are made again only when a frame was sent or failed,
        // as that reads the controller.
        let painted = self.app.borrow_mut().on_num_lock(on);
        if !matches!(painted, Ok(false)) {
            self.act(|_| painted.map(|_| ()));
        }
    }

    /// Paints the mic key again if the mic mute changed since the last
    /// look (`None`: it can't be read). While a resume waits, the state is
    /// only kept: the resume paints it.
    pub fn follow_mic(self: &Rc<Self>, muted: Option<bool>) {
        if self.app.borrow().mic_muted() == muted {
            return;
        }
        if self.resuming.get() > 0 {
            self.app.borrow_mut().set_mic_muted(muted);
            return;
        }
        let painted = self.app.borrow_mut().on_mic_muted(muted);
        if !matches!(painted, Ok(false)) {
            self.act(|_| painted.map(|_| ()));
        }
    }

    /// Fn + mic: mutes or unmutes the default mic, then paints the key. The
    /// mic changes during a resume's pause too; only the paint waits. The
    /// Win + Alt + K the key sends still reaches Windows, so an app in a
    /// call may mute itself too; the key shows only the mic's own mute.
    pub fn toggle_mic(self: &Rc<Self>) {
        // A wake not yet polled must hold the paint too.
        self.follow_own_power();
        if let Some(muted) = P::toggle_mic_mute() {
            self.follow_mic(Some(muted));
        }
    }

    /// Paints the touchpad key after a touchpad change, unless a resume
    /// waits: the resume reads the touchpad and paints it. Shows the
    /// touchpad popup on a change, also while a resume waits.
    pub fn follow_touchpad(self: &Rc<Self>) {
        self.follow_own_power();
        if self.resuming.get() == 0 {
            self.act(blade_lite::App::on_touchpad_changed);
        }
        // Only shown, nothing sent, so also while a resume waits.
        let mut watch = self.touchpad_watch.get();
        let changed = watch.changed(self.app.borrow().touchpad_on());
        self.touchpad_watch.set(watch);
        if let Some(state) = changed {
            self.pop_up(TOUCHPAD, state, POPUP_HOLD);
        }
    }

    /// Takes the power events from now on, for each poll and each click.
    pub fn listen_for_power(&self, power: Receiver<PowerEvent>) {
        self.power.replace(Some(Rc::new(power)));
    }

    /// Acts on the power events that came since the last look, if any are
    /// listened for. If none can come (Windows refused them, or their
    /// thread ended), the power source is read instead.
    pub fn follow_own_power(self: &Rc<Self>) {
        // First, so a new handle's pause holds this click or poll too.
        self.follow_reopen();
        let power = self.power.borrow().clone();
        if let Some(power) = power
            && !self.follow_power(&power)
        {
            self.follow_source();
        }
    }

    /// Acts on the power events that came since the last look. Returns
    /// whether more can come. A sleep with a wake behind it is over: the
    /// app was frozen before it read it, so it sends nothing, as the
    /// controller may not be ready yet; the wake sends after its pause.
    pub fn follow_power(self: &Rc<Self>, power: &Receiver<PowerEvent>) -> bool {
        let mut events = Vec::new();
        let more = loop {
            match power.try_recv() {
                Ok(event) => events.push(event),
                Err(TryRecvError::Empty) => break true,
                Err(TryRecvError::Disconnected) => break false,
            }
        };
        for (at, &event) in events.iter().enumerate() {
            if event == PowerEvent::Suspending && events[at..].contains(&PowerEvent::Resumed) {
                report("the sleep was read only after the wake, so nothing was sent for it");
                continue;
            }
            self.power_event(event);
        }
        more
    }

    /// Passes on a change of the power source read now as its event, for
    /// when no power events come. An unknown read changes nothing, so a
    /// read that flickers to unknown does not switch the preset back and
    /// forth. A wake is not seen then.
    pub(super) fn follow_source(self: &Rc<Self>) {
        let Some(on_battery) = P::on_battery() else {
            return;
        };
        if on_battery == self.app.borrow().on_battery() {
            self.app.borrow_mut().source_read(on_battery);
            return;
        }
        self.power_event(if on_battery {
            PowerEvent::OnBattery
        } else {
            PowerEvent::OnAc
        });
    }

    /// Acts on one power event. A change of source while a resume waits
    /// is dropped and sends nothing: the resume reads the source when its pause ends,
    /// and sends then.
    fn power_event(self: &Rc<Self>, event: PowerEvent) {
        if event == PowerEvent::Suspending {
            self.suspend();
            return;
        }
        if event == PowerEvent::Resumed {
            report("Windows says the laptop woke, so the settings go again after the pause");
            self.wake();
            return;
        }
        self.forget_charging();
        if self.resuming.get() == 0 {
            self.act(|app| app.on_power(event));
        }
    }

    /// After a sleep notice, counts a run of ticks of
    /// [`MISSED_WAKE_AFTER`] with no gap over [`TICK_GAP`] (wall clock,
    /// which counts a freeze), with input at least [`INPUT_GRACE`] into the
    /// run, as a wake: a failed or cancelled sleep may bring no wake notice (not
    /// documented, a safety net). With no input, such as with the lid shut
    /// on a game whose hibernate failed, the sleep goes on: the quit
    /// preset runs and nothing is sent, the safe side. A clock set back or
    /// an unknown input time does the same. Input another program injects
    /// counts too (Windows' last-input time can't tell it apart).
    fn follow_missed_wake(self: &Rc<Self>, wall: SystemTime) {
        if !self.asleep.get() {
            return;
        }
        let began = match self.awake_run.get() {
            Some((began, last)) if wall.duration_since(last).is_ok_and(|gap| gap <= TICK_GAP) => {
                began
            }
            _ => wall,
        };
        self.awake_run.set(Some((began, wall)));
        let Ok(ran) = wall.duration_since(began) else {
            return;
        };
        if ran >= MISSED_WAKE_AFTER
            && P::input_age().is_some_and(|age| age.saturating_add(INPUT_GRACE) < ran)
        {
            report(
                "the app has run for a minute since Windows said the laptop sleeps, with input from the user and no wake notice, so this counts as a wake: the settings go again after the pause",
            );
            self.wake();
        }
    }

    /// At the first tick after a wake's pause, measures the sleep by the
    /// wall clock (`Instant` may not count it): after a
    /// [`LONG_SLEEP`], a fan guard at full speed ends after a short cool
    /// hold (ticket 172), which is logged. A sleep not measured (its notice
    /// came with the wake, or the clock was set back) keeps the usual hold.
    fn follow_long_sleep(&self, wall: SystemTime) {
        if !self.woke.take() {
            return;
        }
        let Some(slept) = self
            .slept_at
            .take()
            .and_then(|at| wall.duration_since(at).ok())
            .filter(|&slept| slept >= LONG_SLEEP)
        else {
            return;
        };
        let mut guard = self.fan_guard.get();
        let on = guard.woke();
        self.fan_guard.set(guard);
        if on && self.app.borrow().manual_fan_runs() {
            report(&format!(
                "fan guard: the laptop slept {} min, so {} s of cool readings end full speed",
                slept.as_secs() / 60,
                FAN_GUARD_WAKE_HOLD.as_secs()
            ));
        }
    }

    /// Ends a sleep, if any, and sends the saved settings after the wake's
    /// pause.
    fn wake(self: &Rc<Self>) {
        self.forget_charging();
        // The sleep's pause ends: the wake's own begins.
        if self.asleep.replace(false) {
            self.resuming.set(self.resuming.get().saturating_sub(1));
        }
        // The last pause sends the wake's settings.
        self.wake_due.set(true);
        // The time asleep may not count in `Instant`, so a spell before
        // the sleep is not carried over.
        self.heat_watch.set(HeatWatch::default());
        // The controller needs a moment after the wake.
        let pause = self.begin_pause();
        let shell = Rc::clone(self);
        Timer::single_shot(RESUME_PAUSE, move || {
            shell.resuming.set(shell.resuming.get().saturating_sub(1));
            // A sleep or a newer pause since: the last pause sends them.
            if !shell.superseded(pause) {
                shell.end_wake_pause();
            }
        });
    }

    /// The end of the last pause after a wake (the wake's own, or an open
    /// again begun in it): sends the saved settings.
    fn end_wake_pause(self: &Rc<Self>) {
        self.wake_due.set(false);
        self.woke.set(true);
        {
            let mut app = self.app.borrow_mut();
            app.set_num_lock(num_lock_on());
            // The charger may have been pulled in the sleep, and the
            // power event may come after this resume.
            app.take_on_battery(P::on_battery());
        }
        let first = self.look_for_razer();
        // This send covers an updater that just ended.
        self.updater_ended.set(false);
        // An open again or an updater whose pause ended in the sleep or
        // a newer pause: what it left for the wake.
        if self.redo_at_wake.take() {
            P::release_controller();
            self.controller_gone.set(false);
            self.app.borrow_mut().check_firmware();
        }
        self.act(|app| app.on_power(PowerEvent::Resumed));
        // The wake is known now: the fan guard reads at the next tick, not
        // one skipped as late by the sleep and 10 s more (ticket 171).
        self.schedule.borrow_mut().read_heat_now();
        if first {
            self.show_window();
        }
    }

    /// Windows says the laptop goes to sleep or hibernates (ticket 169).
    /// On Modern Standby the controller stays powered, so a manual fan or
    /// Performance would run on in the sleep: the quit preset goes at once,
    /// once per sleep, never after the quit. Then nothing is sent until the
    /// wake, which sends the saved settings after its pause.
    ///
    /// Nothing is sent in a pause (after a wake, an open again or an
    /// updater), as no send goes then: the controller may not be ready, and
    /// after an open again its new handle is held (ticket 132). A manual
    /// fan may then run on in the sleep: after a wake, only if the sleep
    /// before it was not sent; after an open again or an updater, if it was
    /// running. Nor for a notice Windows no longer waits for: the app may
    /// have been frozen and thawed since, so the controller may have just
    /// woken. Both are rare (a sleep within 2 s of such an event, or an
    /// event loop busy for over 1 s), and logged. No retry: Windows freezes
    /// the app a few seconds later.
    fn suspend(&self) {
        // A notice while asleep (the last sleep had no wake notice) starts
        // the minute to a missed wake again; nothing was sent since the last.
        self.awake_run.set(None);
        if self.asleep.get() || self.quit_sent.get() {
            return;
        }
        self.slept_at.set(Some(SystemTime::now()));
        self.woke.set(false);
        if self.resuming.get() > 0 {
            report(
                "the laptop goes to sleep in the pause after a wake, an open again or an updater, so the quit preset was not sent: a manual fan or Performance may keep running in the sleep",
            );
        } else if !P::sleep_notice_waits() {
            report(
                "the sleep notice was read after Windows stopped waiting for it (the app was busy), so the quit preset was not sent: a manual fan or Performance may have kept running in the sleep",
            );
        } else {
            self.send_sleep_preset();
        }
        self.asleep.set(true);
        self.resuming.set(self.resuming.get().saturating_add(1));
    }

    /// Sends the quit preset for a sleep, and logs how it went. The windows
    /// are not filled again and no popup shows: the screen is off, and the
    /// wake fills them.
    fn send_sleep_preset(&self) {
        let Ok(mut app) = self.app.try_borrow_mut() else {
            report(
                "the app was busy, so the quit preset was not sent before the sleep: a manual fan or Performance may keep running in it",
            );
            return;
        };
        let began = Instant::now();
        match app.on_power(PowerEvent::Suspending) {
            Ok(()) => report(&format!(
                "the laptop goes to sleep, so the quit preset was sent ({} ms); the settings go again at the wake",
                began.elapsed().as_millis()
            )),
            Err(error) => report(&format!(
                "the quit preset was not sent before the sleep, so a manual fan or Performance may keep running in it: {error}"
            )),
        }
    }

    /// The charging flag read before a power change or a sleep is old: the
    /// pill shows only "Plugged in" until the next poll reads it again.
    fn forget_charging(&self) {
        let mut readings = self.readings.get();
        readings.charging = None;
        self.readings.set(readings);
    }

    /// Reads the sensors, then shows the header and the keyboard group,
    /// since the touchpad key can change it at any time, and watches the
    /// temperatures. The reads that failed are logged once in a while.
    fn tick_readings(self: &Rc<Self>, now: Instant) {
        let (readings, mut errors) = {
            let (sampler, sensors) = &mut *self.sampler.borrow_mut();
            let readings = sampler.sample(sensors);
            self.note_gpu_failed(sampler.gpu_failed());
            (readings, sampler.errors().to_vec())
        };
        self.readings.set(readings);
        let mut app = self.app.borrow_mut();
        let header = self.header(&mut app, readings);
        errors.extend_from_slice(app.read_errors());
        self.say_read_errors(&errors);
        bind::header(&self.window, &header);
        if let Ok(view) = app.keyboard_view() {
            bind::keyboard(&self.window, &view);
        }
        self.show_touchpad(app.touchpad_on());
        let cpu = app.cpu_celsius();
        drop(app);
        let gpu = readings.gpu.map(|gpu| gpu.celsius);
        self.watch_heat(cpu, gpu, now);
    }

    /// Logs the failed reads not logged in the quiet time. While the
    /// controller is closed or held (it died, or waits after an open again),
    /// its read failures are said once, until it is opened again: the open
    /// and its failures are logged on their own (ticket 154).
    pub(crate) fn say_read_errors(&self, errors: &[String]) {
        let (gone, rest): (Vec<String>, Vec<String>) = errors.iter().cloned().partition(|error| {
            [CONTROLLER_CLOSED, CONTROLLER_OPENED_AGAIN]
                .iter()
                .any(|mark| error.contains(mark))
        });
        // Also at most once per quiet time, so a handle that dies at each
        // open again can't log a line each time.
        let floor = [GONE_KEY.to_owned()];
        if let Some(first) = gone.first()
            && !self.controller_gone.replace(true)
            && !self
                .read_errors
                .borrow_mut()
                .to_say(&floor, Instant::now())
                .is_empty()
        {
            report(&format!(
                "read failed: {first} (more such reads are not logged until it is opened again)"
            ));
        }
        for error in self.read_errors.borrow_mut().to_say(&rest, Instant::now()) {
            report(&format!("read failed: {error}"));
        }
    }
}
