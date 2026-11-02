//! The real display list. Reads only, so the screen never changes.

use platform::{DisplayApi, OutputTechnology, WindowsDisplays};

#[test]
#[ignore = "needs a laptop with its own panel turned on"]
fn the_laptop_panel_is_listed_with_its_current_mode() {
    let displays = WindowsDisplays.displays().unwrap();
    for display in &displays {
        eprintln!("{display:?}");
    }

    let panels: Vec<_> = displays
        .iter()
        .filter(|display| display.technology == OutputTechnology::Internal)
        .collect();
    assert_eq!(panels.len(), 1, "expected one internal panel");
    let panel = panels[0];
    assert!(panel.name.starts_with(r"\\.\DISPLAY"), "{}", panel.name);
    assert!(
        panel.modes.contains(&panel.current),
        "current mode not offered"
    );
}
