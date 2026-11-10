//! Power events from Windows. Sleeping or unplugging the laptop is a manual check.

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

#[test]
#[ignore = "needs a laptop, which reports its power source"]
fn the_power_source_comes_once_at_register() {
    use std::sync::mpsc;
    use std::time::Duration;

    use platform::{PowerEvent, PowerEvents};

    let (sender, received) = mpsc::channel();
    std::thread::spawn(move || {
        let mut events = WindowsPowerEvents::register().unwrap();
        let _ = sender.send(events.next_event());
    });

    let first = received.recv_timeout(Duration::from_secs(3)).unwrap();
    eprintln!("first event: {first:?}");
    assert!(matches!(
        first,
        Some(PowerEvent::OnAc | PowerEvent::OnBattery)
    ));
}
