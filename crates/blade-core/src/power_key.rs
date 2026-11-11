//! The power-mode key steps to the next preset, as it did with Synapse.

use std::time::{Duration, Instant};

use razer_proto::{ModelProfile, PowerKey};

use crate::Preset;

/// The shortest time between two steps. Presses closer than this count
/// once, so a bouncing or hammered key can't flood the controller.
pub const POWER_KEY_PAUSE: Duration = Duration::from_millis(300);

/// How long after Fn goes down an Enter still counts. The keyboard sends
/// one report when Fn goes down and one when it comes up, so a lost
/// release (such as over a sleep) would otherwise leave every Enter
/// stepping the preset.
pub const FN_HOLD_LIMIT: Duration = Duration::from_secs(5);

/// How often the keyboard brightness is read while Fn is held, so the
/// app follows Fn + F10 or F11 as the laptop steps the level. Reads stop
/// at [`BRIGHTNESS_HOLD_LIMIT`]; Fn coming up reads once more.
pub const BRIGHTNESS_READ_GAP: Duration = Duration::from_millis(250);

/// How long after Fn goes down the brightness is still read. Longer than
/// [`FN_HOLD_LIMIT`], as Fn may stay held while F11 is pressed many times.
/// A read alone writes nothing; only a changed level sends the vapor
/// light at it. Still a limit, so a lost release can't keep reading for
/// ever.
pub const BRIGHTNESS_HOLD_LIMIT: Duration = Duration::from_secs(30);

/// The preset after `running` in the model's presets, wrapping round. On
/// battery only the presets offered there count. Custom is never stepped
/// to, since it may hold a manual fan speed; from Custom the first preset
/// comes. `None` if there is no preset to step to.
#[must_use]
pub fn next_preset(running: Preset, model: &ModelProfile, on_battery: bool) -> Option<Preset> {
    let cycle: Vec<Preset> = model
        .presets
        .iter()
        .map(|&mode| Preset::from(mode))
        .filter(|preset| !on_battery || preset.offered_on_battery(model))
        .collect();
    let after = cycle
        .iter()
        .position(|&preset| preset == running)
        .map_or(0, |index| index + 1);
    cycle.get(after).or_else(|| cycle.first()).copied()
}

/// Turns the key reports into presses. With Num Lock on the keyboard
/// sends only Fn, and the Enter goes to Windows, so an Enter pressed while
/// Fn is held is a press too (`on_enter`).
#[derive(Debug, Clone)]
pub struct PowerKeyPresses {
    key: PowerKey,
    /// `None` until the first key report. A key held before then is not
    /// a new press: it may have been down while the reports were opened.
    held: Option<bool>,
    /// When Fn went down, while it is held. If a release report is lost,
    /// the next Fn press keeps the old time, so Enter is not watched until
    /// a report without Fn comes: a missed press, never a made-up one.
    fn_since: Option<Instant>,
    /// Enter as last seen while Fn is held. `None` until seen, so an
    /// Enter already down when Fn came is not a press.
    enter: Option<bool>,
    /// The mic keys (Win + Alt + K) as last seen while Fn is held, as
    /// `enter` is.
    mic: Option<bool>,
    /// Whether the last key report let Fn go after a report that held it.
    fn_up: bool,
    /// When the brightness was last due in this Fn hold, or when Fn went
    /// down.
    brightness_at: Option<Instant>,
    last_step: Option<Instant>,
    last_mic: Option<Instant>,
}

impl PowerKeyPresses {
    #[must_use]
    pub fn new(key: PowerKey) -> Self {
        Self {
            key,
            held: None,
            fn_since: None,
            enter: None,
            mic: None,
            fn_up: false,
            brightness_at: None,
            last_step: None,
            last_mic: None,
        }
    }

    /// True if this report is a new press that should step the preset.
    pub fn on_report(&mut self, report: &[u8], now: Instant) -> bool {
        self.fn_up = false;
        let Some(held) = self.key.held(report) else {
            return false;
        };
        self.follow_fn(report, now);
        let pressed = held && self.held == Some(false);
        self.held = Some(held);
        pressed && self.step(now)
    }

    /// Whether the last report let Fn go: Fn + F10 or F11 may then have
    /// changed the keyboard brightness. Those keys send no code of their
    /// own; the laptop changes the level itself while Fn is held.
    #[must_use]
    pub fn fn_came_up(&self) -> bool {
        self.fn_up
    }

    /// Whether the keyboard brightness should be read now: Fn is held,
    /// not past [`BRIGHTNESS_HOLD_LIMIT`], and [`BRIGHTNESS_READ_GAP`] has
    /// passed since Fn went down or since the last read. Noted when true.
    pub fn brightness_due(&mut self, now: Instant) -> bool {
        let held = self
            .fn_since
            .is_some_and(|since| now.saturating_duration_since(since) < BRIGHTNESS_HOLD_LIMIT);
        if !held {
            return false;
        }
        let since = self.brightness_at.or(self.fn_since);
        let due = since.is_some_and(|at| now.saturating_duration_since(at) >= BRIGHTNESS_READ_GAP);
        if due {
            self.brightness_at = Some(now);
        }
        due
    }

    /// Whether Enter should be read now: Fn is held, and not for longer
    /// than [`FN_HOLD_LIMIT`].
    #[must_use]
    pub fn watching_enter(&self, now: Instant) -> bool {
        self.fn_since
            .is_some_and(|since| now.saturating_duration_since(since) < FN_HOLD_LIMIT)
    }

    /// True if Enter, read as `down` now, is a new press while Fn is held,
    /// so it should step the preset.
    pub fn on_enter(&mut self, down: bool, now: Instant) -> bool {
        // The reader checks this too, before it reads Enter; kept here so
        // no caller can step on an Enter outside an Fn hold.
        if !self.watching_enter(now) {
            self.enter = None;
            return false;
        }
        let pressed = down && self.enter == Some(false);
        self.enter = Some(down);
        pressed && self.step(now)
    }

    /// True if the mic keys, read as `down` now, are a new press while Fn
    /// is held: Fn + mic sends Win + Alt + K to Windows, and only Fn in the
    /// key reports. Presses closer than [`POWER_KEY_PAUSE`] count once.
    pub fn on_mic(&mut self, down: bool, now: Instant) -> bool {
        if !self.watching_enter(now) {
            self.mic = None;
            return false;
        }
        let pressed = down && self.mic == Some(false);
        self.mic = Some(down);
        if !pressed || Self::within_pause(self.last_mic, now) {
            return false;
        }
        self.last_mic = Some(now);
        true
    }

    /// Notes when Fn goes down, and forgets Enter and the mic keys when it
    /// comes up.
    fn follow_fn(&mut self, report: &[u8], now: Instant) {
        if self.key.fn_held(report) == Some(true) {
            self.fn_since.get_or_insert(now);
        } else {
            self.fn_up = self.fn_since.is_some();
            self.fn_since = None;
            self.brightness_at = None;
            self.enter = None;
            self.mic = None;
        }
    }

    /// True, and noted, unless the last step was too soon before.
    fn step(&mut self, now: Instant) -> bool {
        if self.too_soon(now) {
            return false;
        }
        self.last_step = Some(now);
        true
    }

    fn too_soon(&self, now: Instant) -> bool {
        Self::within_pause(self.last_step, now)
    }

    fn within_pause(last: Option<Instant>, now: Instant) -> bool {
        last.is_some_and(|last| now.saturating_duration_since(last) < POWER_KEY_PAUSE)
    }
}
