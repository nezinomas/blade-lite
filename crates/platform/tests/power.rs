//! Resume events from Windows. Sleeping the laptop is a manual check.

use platform::WindowsPowerEvents;

#[test]
fn registers_for_resume_events_and_unregisters_on_drop() {
    let events = WindowsPowerEvents::register().unwrap();
    drop(events);
}

#[test]
fn can_be_moved_to_another_thread() {
    let events = WindowsPowerEvents::register().unwrap();
    std::thread::spawn(move || drop(events)).join().unwrap();
}
