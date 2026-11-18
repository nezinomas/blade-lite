//! What the shell does on each poll: power events, Num Lock, the mic mute
//! and sensor reads.

use std::rc::Rc;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Duration, Instant, SystemTime};

use blade_core::{GUARD_HEAT_INTERVAL, HEAT_INTERVAL, RESUME_PAUSE, Readings};
use blade_lite::{AppError, PollTime, StepTime, TouchpadLook, report};
use platform::PowerEvent;
use razer_device::{CONTROLLER_CLOSED, CONTROLLER_OPENED_AGAIN};
use slint::{ComponentHandle, Timer};
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

/// How many times a keyboard put-back after a screen on is tried before it
/// is given up until the screen goes off again, a wake or the quit, and the
/// least time between two tries (ticket 253): as the lighting after a
/// charger change (`SOURCE_LIGHTING_TRIES`, `SOURCE_LIGHTING_SPACING`).
const PUT_BACK_TRIES: u8 = blade_lite::SOURCE_LIGHTING_TRIES;
const PUT_BACK_SPACING: Duration = blade_lite::SOURCE_LIGHTING_SPACING;

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

/// How a change run through `act`, or a charger change, sent the guard's
/// failed full speed, for its log line (ticket 180).
pub(crate) const WITH_THE_PRESET: &str = "with the preset";

/// How a poll's resend sent the guard's failed full speed, for its log
/// line (ticket 163).
pub(crate) const BY_A_POLL: &str = "by a poll";

/// What Windows last said of the lid and the screen, for the log (ticket
/// 184).
fn lid_and_screen(lid_open: Option<bool>, screen_on: Option<bool>) -> String {
    format!(
        "lid {}, screen {}",
        said(lid_open, "open", "shut"),
        said(screen_on, "on", "off")
    )
}

/// What Windows said of the lid or the screen: `yes` or `no`, or "not
/// known" if it said nothing.
fn said(state: Option<bool>, yes: &'static str, no: &'static str) -> &'static str {
    match state {
        Some(true) => yes,
        Some(false) => no,
        None => "not known",
    }
}

#[cfg(test)]
thread_local! {
    /// How each `Full speed, sent again` line this test's thread logged
    /// says the send went: the window tests share one log.
    pub static FULL_SPEED_SENT_AGAIN: std::cell::RefCell<Vec<&'static str>> =
        const { std::cell::RefCell::new(Vec::new()) };
    /// How many missed wakes this test's thread logged as held (ticket 184).
    pub static MISSED_WAKES_HELD: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
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
        self.tick_now.set(now);
        let failures = self.app.borrow().preset_failures();
        let looked = self.look_for_razer_now_and_then(now);
        // Before any send: an updater end just found waits its pause.
        self.start_after_updater();
        self.follow_own_power();
        self.follow_missed_wake(wall);
        self.follow_long_sleep(wall);
        self.send_fan_targets_again(now);
        self.app.borrow_mut().keep_resending(now);
        if looked {
            self.read_firmware_if_unread();
        }
        if self.resuming.get() == 0 && self.app.borrow().battery_rate_pending() {
            self.retry_battery_rate();
        }
        self.follow_num_lock(P::num_lock_on());
        self.log_touchpad_sources();
        // A failed read keeps the last state, so a flaky read can't
        // send a frame each poll.
        if let Some(muted) = P::mic_muted() {
            self.follow_mic(Some(muted));
        }
        self.follow_views_in_sight(now);
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

    /// Tells the poll schedule whether the screen is off (Windows' notice;
    /// unknown counts as on) and the window minimised: a view out of sight
    /// reads as with no view, so the fan guard's reads go on (ticket 188).
    fn follow_views_in_sight(&self, now: Instant) {
        let minimized = self.window.window().is_minimized();
        let mut schedule = self.schedule.borrow_mut();
        schedule.set_screen_off(P::screen_on() == Some(false), now);
        schedule.set_window_minimized(minimized, now);
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
                // An updater end it found waits its pause (ticket 179).
                self.start_after_updater();
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
                    // An updater that just ended: its own pause sends.
                    if shell.look_then_wait_for_updater() {
                        return;
                    }
                    // This start covers an older pause that left its work
                    // to this one.
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
                    // A screen change in the pause was dropped.
                    shell.follow_screen();
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
    /// One that ends a manual fan the window does not show fills the
    /// windows again, so its warning goes, with the old error (ticket 250).
    /// A failed one is shown. If the guard's own full-speed send failed,
    /// its log line said nothing went: a resend that takes it says so
    /// (ticket 163).
    fn resend_preset(self: &Rc<Self>) {
        let quiet = self.app.borrow().preset_failures() == self.preset_failures_seen.get();
        if !quiet || !self.heat_reads_allowed() {
            return;
        }
        let unsent = self.app.borrow().boost_unsent();
        let unshown = self.app.borrow().manual_fan_unshown();
        let sent = self.app.borrow_mut().resend_preset();
        self.say_full_speed_sent_again(unsent, BY_A_POLL);
        // Its warning goes once a resend ends it (ticket 250). The fill
        // drops the failed send's error too: a whole send went since.
        if unshown && matches!(sent, Ok(true)) && !self.app.borrow().manual_fan_unshown() {
            self.refresh(None);
        }
        if let Err(error) = sent {
            self.act(|_| Err(error));
        }
    }

    /// Logs that the guard's failed full-speed send went `how`, if it was
    /// `unsent` before a send that took it: its own log line said nothing
    /// went (tickets 163, 180).
    pub(super) fn say_full_speed_sent_again(&self, unsent: bool, how: &'static str) {
        if unsent && !self.app.borrow().boost_unsent() {
            report(&format!("fan guard: Full speed, sent again {how}"));
            #[cfg(test)]
            FULL_SPEED_SENT_AGAIN.with_borrow_mut(|lines| lines.push(how));
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
        let unsent = self.app.borrow().boost_unsent();
        let changed = self.app.borrow_mut().follow_charger();
        let missed = self.app.borrow_mut().take_missed_reads();
        self.say_read_errors(&missed);
        self.say_full_speed_sent_again(unsent, WITH_THE_PRESET);
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
        let view = self.steps.time("display view", || {
            if self.heat_reads_allowed() {
                app.display_view()
            } else {
                app.display_view_unread()
            }
        });
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
            self.controller_read(|| app.read_cpu_temperature());
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

    /// Logs what the registry and Windows' touchpad API say, with Num Lock
    /// and how the touchpad key was last painted, when one changes (ticket
    /// 239). Only reads and logs.
    fn log_touchpad_sources(&self) {
        let look = {
            let app = self.app.borrow();
            TouchpadLook {
                registry: app.touchpad_on(),
                api: P::touchpad_api_on(),
                num_lock: app.num_lock_on(),
                key_red: app.touchpad_key_red(),
            }
        };
        let mut sources = self.touchpad_sources.get();
        let line = sources.line(look, self.tick_now.get());
        self.touchpad_sources.set(sources);
        if let Some(line) = line {
            report(&line);
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
        self.follow_source_lighting();
        self.follow_screen();
    }

    /// Sends the saved lighting once after the charger changes this look
    /// took (ticket 244), at most once a
    /// [`SOURCE_LIGHTING_SPACING`](blade_lite::SOURCE_LIGHTING_SPACING)
    /// (ticket 246), so a bouncing plug can't send it at each poll. Dropped
    /// in a pause (its end sends all settings), while Razer's software runs
    /// (it may write its own lighting then) and while an updater holds the
    /// controller (its end sends all settings).
    fn follow_source_lighting(self: &Rc<Self>) {
        // No pause and no Razer software: the same rule as the heat reads,
        // kept in step on purpose (both stay off the controller then).
        let allowed = self.heat_reads_allowed();
        let Ok(mut app) = self.app.try_borrow_mut() else {
            // Busy: the next poll or click looks again.
            return;
        };
        if !app.source_lighting_due() {
            return;
        }
        if !allowed || app.controller_held() {
            app.forget_source_lighting();
            return;
        }
        // The last tick's clock: a click between ticks sees at most 1 s
        // less time gone, so it waits longer, never less.
        app.send_source_lighting(self.tick_now.get());
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
            // The sleep notice forgot the screen, so it would read as on
            // and light keys made dark just before the sleep (ticket 241).
            if event == PowerEvent::ScreenChanged && events[at..].contains(&PowerEvent::Suspending)
            {
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

    /// Acts on one power event. A change of source or of the screen while
    /// a resume waits is dropped and sends nothing: the resume reads them
    /// when its pause ends, and sends then.
    fn power_event(self: &Rc<Self>, event: PowerEvent) {
        if event == PowerEvent::Suspending {
            self.suspend();
            return;
        }
        if event == PowerEvent::Resumed {
            report(&format!(
                "Windows says the laptop woke ({}), so the settings go again after the pause",
                lid_and_screen(P::lid_open(), P::screen_on())
            ));
            self.wake();
            return;
        }
        if event == PowerEvent::ScreenChanged {
            self.follow_screen();
            return;
        }
        self.forget_charging();
        if self.resuming.get() == 0 {
            self.act(|app| app.on_power(event));
        }
    }

    /// Makes the keys dark while Windows says the screen is off, and lights
    /// them again once it is on or not known (ticket 241). A screen not
    /// known (after a sleep notice, or with no screen notices) counts as
    /// on, so keys the app made dark are never left so. Nothing in a
    /// pause or a sleep: each pause's end looks again, and so does each
    /// poll and click. A failed put-back is tried again at a look
    /// [`PUT_BACK_SPACING`] later, [`PUT_BACK_TRIES`] times in all, then
    /// given up until the screen goes off again or a wake (each one may
    /// have reached the controller, ticket 253); the quit still tries.
    /// The keys are not made dark while Razer's own software runs (it may
    /// talk to the controller at the same time); keys already dark are
    /// still lit. An error is only logged: the user did not ask for this,
    /// and the window is not filled again for it.
    pub(super) fn follow_screen(self: &Rc<Self>) {
        if self.resuming.get() > 0 {
            return;
        }
        let screen_off = P::screen_on() == Some(false);
        // A screen off gives a put-back given up its tries again.
        if screen_off {
            self.put_back_failed.set((0, None));
        }
        let razer_runs = self.razer_warning.borrow().is_some();
        if screen_off && razer_runs {
            return;
        }
        let Ok(mut app) = self.app.try_borrow_mut() else {
            // Busy: the next poll or click looks again.
            return;
        };
        // Its own lighting may read 0: no guess at the app's dark then.
        if razer_runs {
            app.forget_dark_slot();
        }
        // Held for an updater: its end looks again.
        if app.screen_followed(screen_off) || app.controller_held() {
            return;
        }
        let now = self.tick_now.get().max(Instant::now());
        let (failures, tried) = self.put_back_failed.get();
        if !screen_off && failures >= PUT_BACK_TRIES
            || tried.is_some_and(|at| now.saturating_duration_since(at) < PUT_BACK_SPACING)
        {
            return;
        }
        let followed = app.follow_screen(screen_off);
        if !screen_off && followed.is_err() {
            self.put_back_failed.set((failures + 1, Some(now)));
            if failures + 1 == PUT_BACK_TRIES {
                report(&format!(
                    "the keyboard put-back failed {PUT_BACK_TRIES} times, so it waits for the screen to go off and on again, a wake or the quit"
                ));
            }
        }
        match followed {
            Ok(()) => {
                self.screen_error_logged.set(None);
                self.put_back_failed.set((0, None));
            }
            // Once per screen state until a look works: a failed put-back
            // is tried each poll.
            Err(error)
                if self.screen_error_logged.replace(Some(screen_off)) != Some(screen_off) =>
            {
                report(&format!(
                    "the keyboard could not follow the screen: {error}"
                ));
            }
            Err(_) => {}
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
    /// counts too (Windows' last-input time can't tell it apart), as does
    /// an external mouse or keyboard, so Windows must also have said the
    /// screen is on since the notice, and not that the lid is shut (ticket
    /// 184): a screen on from before the sleep may be stale, a shut lid can
    /// light the screen in a failed hibernate, and a lid open with the
    /// screen off is a power-button standby. Otherwise the sleep goes on,
    /// logged once per sleep notice. The price: a sleep cancelled before
    /// the screen went off brings no screen notice, so it waits for the
    /// screen to go off and on again, or for a wake notice.
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
        if ran < MISSED_WAKE_AFTER
            || P::input_age().is_none_or(|age| age.saturating_add(INPUT_GRACE) >= ran)
        {
            return;
        }
        // Read once, so the log shows what was decided on.
        let (lid_open, screen_on) = (P::lid_open(), P::screen_on());
        let seen = lid_and_screen(lid_open, screen_on);
        if screen_on != Some(true) || lid_open == Some(false) {
            if !self.missed_wake_held.replace(true) {
                report(&format!(
                    "the app has run for a minute since Windows said the laptop sleeps, with input from the user and no wake notice, but not with the screen on since the notice and the lid not shut ({seen}), so the sleep goes on: nothing is sent until the screen goes off and on again or Windows says the laptop woke"
                ));
                #[cfg(test)]
                MISSED_WAKES_HELD.set(MISSED_WAKES_HELD.get() + 1);
            }
            return;
        }
        report(&format!(
            "the app has run for a minute since Windows said the laptop sleeps, with input from the user and no wake notice ({seen}), so this counts as a wake: the settings go again after the pause"
        ));
        // The sleep failed: the chips did not cool (ticket 172).
        self.slept_at.set(None);
        self.wake();
    }

    /// At the first tick after a wake's pause, measures the sleep by the
    /// wall clock (`Instant` may not count it): after a
    /// [`LONG_SLEEP`], a fan guard at full speed ends after a short cool
    /// hold (ticket 172), which is logged. A sleep not measured (its notice
    /// came with the wake, a missed wake, or the clock was set back) keeps
    /// the usual hold. A clock set forward in the sleep is believed: the
    /// short hold still needs its cool readings. Only for a manual fan, the
    /// one the guard speeds up.
    fn follow_long_sleep(&self, wall: SystemTime) {
        if !self.woke.take() {
            return;
        }
        let slept_at = self.slept_at.take();
        if !self.app.borrow().manual_fan_runs() {
            return;
        }
        let Some(slept) = slept_at
            .and_then(|at| wall.duration_since(at).ok())
            .filter(|&slept| slept >= LONG_SLEEP)
        else {
            return;
        };
        let mut guard = self.fan_guard.get();
        let on = guard.woke();
        self.fan_guard.set(guard);
        // Nothing is sent while Razer software runs.
        if on && self.heat_reads_allowed() {
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
        // The wake lights the keys again, with its own tries.
        self.put_back_failed.set((0, None));
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
    /// again or an updater's begun in it): sends the saved settings. An
    /// updater found ended then gets its own pause first (ticket 179).
    pub(super) fn end_wake_pause(self: &Rc<Self>) {
        // An updater that just ended: its own pause sends.
        if self.look_then_wait_for_updater() {
            return;
        }
        self.wake_due.set(false);
        self.woke.set(true);
        {
            let mut app = self.app.borrow_mut();
            app.set_num_lock(P::num_lock_on());
            // The mic is read again at the next tick, not only on notices.
            P::mic_read_again();
            // The charger may have been pulled in the sleep, and the
            // power event may come after this resume.
            app.take_on_battery(P::on_battery());
        }
        // An open again or an updater whose pause ended in the sleep or
        // a newer pause: what it left for the wake.
        if self.redo_at_wake.take() {
            P::release_controller();
            self.controller_gone.set(false);
            self.app.borrow_mut().check_firmware();
        }
        self.act(|app| app.on_power(PowerEvent::Resumed));
        // After the settings, which may light the keys: a screen that went
        // off or on in the pauses.
        self.follow_screen();
        // The wake is known now: the fan guard reads at the next tick, not
        // one skipped as late by the sleep and 10 s more (ticket 171).
        self.schedule.borrow_mut().read_heat_now();
    }

    /// Windows says the laptop goes to sleep or hibernates (ticket 169).
    /// On Modern Standby the controller stays powered, so a manual fan or
    /// Performance would run on in the sleep: the quit preset goes at once,
    /// once per sleep (see below for a new notice), never after the quit. Then nothing is sent until the
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
    /// event loop busy for over 1 s), and logged. No retry for the same
    /// notice: Windows freezes the app a few seconds later.
    ///
    /// A quit preset that did not go (failed or not sent) goes again at a
    /// new notice before the wake, under the same rules (ticket 191): a
    /// sleep Windows cancels with no wake notice can be followed by a real
    /// one. This breaks the rule that nothing is sent twice on purpose: the
    /// quit preset sent again changes nothing, and ends a manual fan. It
    /// goes with no 2 s pause, though Windows may have just thawed the app
    /// (not checked on the laptop); the replies are checked, so at worst
    /// it fails again. After a lost handle it only opens a new one, held
    /// until the wake, so nothing goes. A failure does not mark the preset
    /// unsure: nothing else is sent while asleep, and the wake, a missed one
    /// too, sends the whole saved preset again.
    ///
    /// Each send reads the source first, as a quit does (ticket 199): a
    /// charger pulled with no power event read for it moves the app to
    /// battery and gets the battery quit preset. That read can mark the
    /// preset unsure, which nothing reads while asleep; the wake's start
    /// clears it. Otherwise the source is not followed while asleep, so a
    /// quit then still sends the fans on auto first if a manual Custom ran
    /// on AC before the sleep.
    fn suspend(&self) {
        // A notice while asleep (the last sleep had no wake notice) starts
        // the minute to a missed wake again; only a quit preset that did
        // not go is sent again.
        self.awake_run.set(None);
        self.missed_wake_held.set(false);
        // From the last notice: the app ran since an earlier one.
        self.slept_at.set(Some(SystemTime::now()));
        self.woke.set(false);
        let again = self.asleep.get();
        if (again && self.sleep_preset_sent.get()) || self.quit_sent.get() {
            return;
        }
        if !again {
            self.sleep_preset_sent.set(false);
        }
        // The sleep's own count is no pause.
        if self.resuming.get() > u32::from(again) {
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
        if !again {
            self.asleep.set(true);
            self.resuming.set(self.resuming.get().saturating_add(1));
        }
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
        Self::battery_read_for_quit_preset(&mut app);
        let began = Instant::now();
        match app.on_power(PowerEvent::Suspending) {
            Ok(()) => {
                self.sleep_preset_sent.set(true);
                report(&format!(
                    "the laptop goes to sleep, so the quit preset was sent ({} ms, lid {}); the settings go again at the wake",
                    began.elapsed().as_millis(),
                    said(P::lid_open(), "open", "shut")
                ));
            }
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
        let header = self.controller_read(|| self.header(&mut app, readings));
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

    /// Runs `read`, adding its time to the controller reads' (ticket 206).
    fn controller_read<T>(&self, read: impl FnOnce() -> T) -> T {
        let began = Instant::now();
        let value = read();
        self.controller_time
            .set(self.controller_time.get().saturating_add(began.elapsed()));
        value
    }

    /// The time the GPU, the other sensors and the controller's sensor
    /// reads took since the last take, for the slow poll log (ticket 206).
    /// A sampler borrowed elsewhere gives no sensor times: only a log.
    pub fn take_read_times(&self) -> PollTime {
        let (gpu, sensors) = self
            .sampler
            .try_borrow_mut()
            .map(|mut sampler| sampler.1.take_times())
            .unwrap_or_default();
        PollTime {
            gpu,
            sensors,
            controller: self.controller_time.take(),
            ..PollTime::default()
        }
    }

    /// The work timed on the event loop since the last take (ticket 214).
    pub fn take_steps(&self) -> StepTime {
        self.steps.take()
    }

    /// Logs the failed reads not logged in the quiet time. While the
    /// controller is closed or held (it died, or waits after an open again),
    /// its read failures are said once, until it is opened again: the open
    /// and its failures are logged on their own (ticket 154). The quiet time
    /// runs on the last tick's clock, or the real one if that is later (a
    /// fill between ticks, or the first after a wake).
    pub(crate) fn say_read_errors(&self, errors: &[String]) {
        let now = self.tick_now.get().max(Instant::now());
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
            && !self.read_errors.borrow_mut().to_say(&floor, now).is_empty()
        {
            report(&format!(
                "read failed: {first} (more such reads are not logged until it is opened again)"
            ));
        }
        for error in self.read_errors.borrow_mut().to_say(&rest, now) {
            report(&format!("read failed: {error}"));
        }
    }
}
