//! What the shell does on each poll: power events, Num Lock, the mic mute
//! and sensor reads.

use std::rc::Rc;
use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::{Instant, SystemTime};

use blade_core::{RESUME_PAUSE, Readings};
use blade_lite::report;
use platform::{PowerEvent, num_lock_on};
use slint::Timer;
use ui::{Header, HeatWatch};

use super::popup::{POPUP_HOLD, TOUCHPAD};
use super::{AppOn, Parts, Shell};
use crate::bind;

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
        self.follow_own_power();
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
            if !self.follow_charger() {
                self.resend_preset();
            }
        } else if self.heat_reads_allowed() && self.schedule.borrow_mut().heat_due(now, wall) {
            self.tick_heat(now);
            if !self.follow_charger() {
                self.resend_preset();
            }
        }
        self.preset_failures_seen.set(failures);
    }

    /// Sends the preset that runs again if its last send failed, so the
    /// laptop does not keep running another one (such as Performance on
    /// battery) until the next power event. Not while a resume waits or
    /// Razer's software runs, as for the heat reads, nor in the tick of a
    /// failed send or the one after it: the controller may still run it.
    /// A resend that works is quiet, so the error of the failed send stays
    /// shown (it may not be the preset's), unless a rate error was held
    /// behind that send: it is shown then, with the windows filled again.
    /// A failed one is shown.
    fn resend_preset(self: &Rc<Self>) {
        let quiet = self.app.borrow().preset_failures() == self.preset_failures_seen.get();
        if !quiet || !self.heat_reads_allowed() {
            return;
        }
        let sent = self.app.borrow_mut().resend_preset();
        if let Err(error) = sent {
            self.act(|_| Err(error));
        }
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

    /// Whether the controller may be read in the background: not while a
    /// resume waits (it may not be ready), nor while Razer's own software
    /// runs (it may talk to the controller at the same time).
    fn heat_reads_allowed(&self) -> bool {
        self.resuming.get() == 0 && self.razer_warning.borrow().is_none()
    }

    /// Reads only the temperatures, for the heat warning.
    fn tick_heat(self: &Rc<Self>, now: Instant) {
        let (gpu, mut errors) = {
            let (sampler, sensors) = &mut *self.sampler.borrow_mut();
            (sampler.gpu_celsius(sensors), sampler.errors().to_vec())
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
    /// the flyout is filled when it opens.
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
        if let Ok(view) = app.display_view() {
            bind::display(&self.window, &view);
        }
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
        let power = self.power.borrow().clone();
        if let Some(power) = power
            && !self.follow_power(&power)
        {
            self.follow_source();
        }
    }

    /// Acts on the power events that came since the last look. Returns
    /// whether more can come.
    pub fn follow_power(self: &Rc<Self>, power: &Receiver<PowerEvent>) -> bool {
        loop {
            match power.try_recv() {
                Ok(event) => self.power_event(event),
                Err(TryRecvError::Empty) => return true,
                Err(TryRecvError::Disconnected) => return false,
            }
        }
    }

    /// Passes on a change of the power source read now as its event, for
    /// when no power events come. An unknown read changes nothing, so a
    /// read that flickers to unknown does not switch the preset back and
    /// forth. A wake is not seen then.
    fn follow_source(self: &Rc<Self>) {
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
        self.forget_charging();
        if event != PowerEvent::Resumed && self.resuming.get() > 0 {
            return;
        }
        if event == PowerEvent::Resumed {
            // The time asleep may not count in `Instant`, so a spell before
            // the sleep is not carried over.
            self.heat_watch.set(HeatWatch::default());
            // The controller needs a moment after the wake.
            self.resuming.set(self.resuming.get().saturating_add(1));
            let shell = Rc::clone(self);
            Timer::single_shot(RESUME_PAUSE, move || {
                shell.resuming.set(shell.resuming.get().saturating_sub(1));
                {
                    let mut app = shell.app.borrow_mut();
                    app.set_num_lock(num_lock_on());
                    // The charger may have been pulled in the sleep, and the
                    // power event may come after this resume.
                    app.take_on_battery(P::on_battery());
                }
                let first = shell.look_for_razer();
                shell.act(|app| app.on_power(PowerEvent::Resumed));
                if first {
                    shell.show_window();
                }
            });
        } else {
            self.act(|app| app.on_power(event));
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
            (sampler.sample(sensors), sampler.errors().to_vec())
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

    /// Logs the failed reads not logged in the quiet time.
    pub(super) fn say_read_errors(&self, errors: &[String]) {
        for error in self.read_errors.borrow_mut().to_say(errors, Instant::now()) {
            report(&format!("read failed: {error}"));
        }
    }
}
