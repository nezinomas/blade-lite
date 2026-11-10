//! What runs on this PC. Reads only.

use platform::{Programs, WindowsPrograms};

#[test]
fn the_running_programs_and_services_are_read() {
    let names = WindowsPrograms.running().unwrap();

    let this = std::env::current_exe().unwrap();
    let this = this.file_name().unwrap().to_string_lossy();
    assert!(
        names.iter().any(|name| *name == this),
        "{this} in {names:?}"
    );
    // The Windows event log service always runs, by name and display name.
    assert!(names.iter().any(|name| name == "EventLog"), "{names:?}");
    assert!(
        names.iter().any(|name| name == "Windows Event Log"),
        "{names:?}"
    );
}
