//! The refresh rates on offer, over a fake display list.

use std::io;

use blade_core::{DisplayError, RefreshRate, refresh_rates, set_refresh_rate};
use platform::{Display, DisplayApi, DisplayMode, OutputTechnology};

const fn mode(width: u32, height: u32, hz: u32) -> DisplayMode {
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

fn panel(current: DisplayMode) -> Display {
    Display {
        name: r"\\.\DISPLAY1".into(),
        technology: OutputTechnology::Internal,
        current,
        modes: PANEL_MODES.to_vec(),
    }
}

fn monitor() -> Display {
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
struct FakeDisplays {
    /// `None` means the query fails.
    displays: Option<Vec<Display>>,
    /// Each display name and the mode it was asked to take.
    set: Vec<(String, DisplayMode)>,
}

impl FakeDisplays {
    fn new(displays: Option<Vec<Display>>) -> Self {
        Self {
            displays,
            set: Vec::new(),
        }
    }
}

impl DisplayApi for FakeDisplays {
    fn displays(&self) -> io::Result<Vec<Display>> {
        self.displays.clone().ok_or(io::ErrorKind::Other.into())
    }

    fn set_mode(&mut self, name: &str, mode: DisplayMode) -> io::Result<()> {
        self.set.push((name.into(), mode));
        Ok(())
    }
}

fn rates(hz: &[u32]) -> Vec<RefreshRate> {
    hz.iter().copied().map(RefreshRate).collect()
}

#[test]
fn the_4k_mode_offers_the_rates_of_its_resolution() {
    let api = FakeDisplays::new(Some(vec![panel(mode(3840, 2400, 240))]));

    assert_eq!(refresh_rates(&api).unwrap(), rates(&[60, 120, 240]));
}

#[test]
fn the_fhd_mode_offers_the_rates_of_its_resolution() {
    let api = FakeDisplays::new(Some(vec![panel(mode(1920, 1200, 440))]));

    assert_eq!(refresh_rates(&api).unwrap(), rates(&[60, 120, 240, 440]));
}

#[test]
fn the_panel_is_picked_by_technology_not_by_order() {
    let api = FakeDisplays::new(Some(vec![monitor(), panel(mode(3840, 2400, 60))]));

    assert_eq!(refresh_rates(&api).unwrap(), rates(&[60, 120, 240]));
}

#[test]
fn each_rate_is_offered_once_and_in_order() {
    let mut display = panel(mode(3840, 2400, 60));
    // Windows lists a mode once per colour depth, in no set order.
    display.modes = vec![
        mode(3840, 2400, 240),
        mode(3840, 2400, 60),
        mode(3840, 2400, 240),
        mode(3840, 2400, 60),
    ];
    let api = FakeDisplays::new(Some(vec![display]));

    assert_eq!(refresh_rates(&api).unwrap(), rates(&[60, 240]));
}

#[test]
fn no_internal_panel_is_an_error() {
    let api = FakeDisplays::new(Some(vec![monitor()]));

    assert!(matches!(
        refresh_rates(&api),
        Err(DisplayError::NoInternalPanel)
    ));
}

#[test]
fn a_failed_query_is_an_error() {
    let api = FakeDisplays::new(None);

    assert!(matches!(refresh_rates(&api), Err(DisplayError::Query(_))));
}

#[test]
fn setting_a_rate_keeps_the_resolution() {
    let mut api = FakeDisplays::new(Some(vec![monitor(), panel(mode(3840, 2400, 240))]));

    set_refresh_rate(&mut api, RefreshRate(60)).unwrap();

    assert_eq!(
        api.set,
        vec![(r"\\.\DISPLAY1".into(), mode(3840, 2400, 60))]
    );
}

#[test]
fn an_unsupported_rate_is_an_error_and_sets_nothing() {
    // 440 Hz is offered only at 1920×1200, not at the current 4K.
    let mut api = FakeDisplays::new(Some(vec![panel(mode(3840, 2400, 240))]));

    let result = set_refresh_rate(&mut api, RefreshRate(440));

    assert!(matches!(
        result,
        Err(DisplayError::Unsupported(RefreshRate(440)))
    ));
    assert!(api.set.is_empty());
}

#[test]
fn setting_a_rate_without_the_panel_is_an_error() {
    let mut api = FakeDisplays::new(Some(vec![monitor()]));

    assert!(matches!(
        set_refresh_rate(&mut api, RefreshRate(60)),
        Err(DisplayError::NoInternalPanel)
    ));
    assert!(api.set.is_empty());
}
