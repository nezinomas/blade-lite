//! A fake display list shared by the display tests.

#![allow(dead_code)]

use std::io;

use platform::{Display, DisplayApi, DisplayMode, ModeLife, OutputTechnology};

pub const fn mode(width: u32, height: u32, hz: u32) -> DisplayMode {
    DisplayMode { width, height, hz }
}

/// What the Blade 18 panel reports, in both native modes' resolutions.
const PANEL_MODES: [DisplayMode; 7] = [
    mode(3840, 2400, 60),
    mode(3840, 2400, 120),
    mode(3840, 2400, 240),
    mode(1920, 1200, 60),
    mode(1920, 1200, 120),
    mode(1920, 1200, 240),
    mode(1920, 1200, 440),
];

pub fn panel(current: DisplayMode) -> Display {
    Display {
        name: r"\\.\DISPLAY1".into(),
        technology: OutputTechnology::Internal,
        current,
        modes: PANEL_MODES.to_vec(),
    }
}

pub fn monitor() -> Display {
    Display {
        name: r"\\.\DISPLAY2".into(),
        technology: OutputTechnology::External,
        current: mode(2560, 1440, 165),
        modes: vec![
            mode(2560, 1440, 60),
            mode(2560, 1440, 144),
            mode(2560, 1440, 165),
        ],
    }
}

/// The displays Windows reports, and the modes asked of it.
pub struct FakeDisplays {
    /// `None` means the query fails.
    pub displays: Option<Vec<Display>>,
    /// Each display name, the mode it was asked to take, and for how long.
    pub set: Vec<(String, DisplayMode, ModeLife)>,
    /// How many of the next sets fail, as Windows refuses one just after a
    /// wake.
    pub failing_sets: usize,
    /// The mode Windows has saved for every display, which a restart
    /// starts at.
    pub saved: Option<DisplayMode>,
}

impl FakeDisplays {
    pub fn new(displays: Option<Vec<Display>>) -> Self {
        Self {
            displays,
            set: Vec::new(),
            failing_sets: 0,
            saved: None,
        }
    }
}

impl DisplayApi for FakeDisplays {
    fn saved_mode(&self, _name: &str) -> Option<DisplayMode> {
        self.saved
    }

    fn displays(&self) -> io::Result<Vec<Display>> {
        self.displays.clone().ok_or(io::ErrorKind::Other.into())
    }

    fn set_mode(&mut self, name: &str, mode: DisplayMode, life: ModeLife) -> io::Result<()> {
        if self.failing_sets > 0 {
            self.failing_sets -= 1;
            return Err(io::ErrorKind::Other.into());
        }
        self.set.push((name.into(), mode, life));
        // The display now runs in the new mode, as on Windows.
        let displays = self.displays.iter_mut().flatten();
        for display in displays.filter(|display| display.name == name) {
            display.current = mode;
        }
        Ok(())
    }
}

impl FakeDisplays {
    /// The panel's current mode.
    pub fn panel_mode(&self) -> DisplayMode {
        let displays = self.displays.as_ref().unwrap();
        let panel = displays
            .iter()
            .find(|display| display.technology == OutputTechnology::Internal);
        panel.unwrap().current
    }
}
