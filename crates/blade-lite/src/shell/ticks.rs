//! What the shell does on each poll: power events, Num Lock and sensor reads.

use std::rc::Rc;
use std::sync::mpsc::Receiver;
use std::time::Instant;

use blade_core::RESUME_PAUSE;
use blade_lite::report;
use platform::{PowerEvent, num_lock_on};
use slint::Timer;

use super::{Parts, Shell};
use crate::bind;

impl<P: Parts> Shell<P> {
    /// Passes on power events and Num Lock, then reads the sensors if a
    /// view shows them.
    pub fn tick(self: &Rc<Self>) {
        self.follow_own_power();
        self.follow_num_lock(num_lock_on());
        if self.schedule.borrow_mut().poll_due(Instant::now()) {
            self.tick_readings();
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

    /// Paints the touchpad key after a touchpad change, unless a resume
    /// waits: the resume reads the touchpad and paints it.
    pub fn follow_touchpad(self: &Rc<Self>) {
        if self.resuming.get() == 0 {
            self.act(blade_lite::App::on_touchpad_changed);
        }
    }

    /// Takes the power events from now on, for each poll and each click.
    pub fn listen_for_power(&self, power: Receiver<PowerEvent>) {
        self.power.replace(Some(Rc::new(power)));
    }

    /// Acts on the power events that came since the last look, if any are
    /// listened for.
    pub fn follow_own_power(self: &Rc<Self>) {
        let power = self.power.borrow().clone();
        if let Some(power) = power {
            self.follow_power(&power);
        }
    }

    /// Acts on the power events that came since the last look.
    pub fn follow_power(self: &Rc<Self>, power: &Receiver<PowerEvent>) {
        for event in power.try_iter() {
            self.power_event(event);
        }
    }

    fn power_event(self: &Rc<Self>, event: PowerEvent) {
        if event == PowerEvent::Resumed {
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

    /// Reads the sensors, then shows the header and the keyboard group,
    /// since the touchpad key can change it at any time. The reads that
    /// failed are logged once in a while.
    fn tick_readings(&self) {
        let (readings, mut errors) = {
            let (sampler, sensors) = &mut *self.sampler.borrow_mut();
            (sampler.sample(sensors), sampler.errors().to_vec())
        };
        self.readings.set(readings);
        let mut app = self.app.borrow_mut();
        let header = app.header(readings);
        errors.extend_from_slice(app.read_errors());
        self.say_read_errors(&errors);
        bind::header(&self.window, &header);
        if let Ok(view) = app.keyboard_view() {
            bind::keyboard(&self.window, &view);
        }
    }

    /// Logs the failed reads not logged in the quiet time.
    pub(super) fn say_read_errors(&self, errors: &[String]) {
        for error in self.read_errors.borrow_mut().to_say(errors, Instant::now()) {
            report(&format!("read failed: {error}"));
        }
    }
}
