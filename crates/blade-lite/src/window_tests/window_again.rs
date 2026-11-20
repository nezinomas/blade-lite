//! Ticket 303: a closed settings window is made again, so the old one's
//! picture (about 12 MB at 200 %) leaves memory. The window made again
//! looks and works as the old one, and making it sends nothing.

use std::time::Duration;

use blade_core::Config;
use i_slint_backend_testing::AccessibleRole::Button;
use razer_proto::{BLADE_18, PerfMode};
use slint::platform::WindowEvent;
use slint::{ComponentHandle, PhysicalSize};

use super::{Fixture, click, fixture, selected_preset};

/// The user closes the window (its X): Slint hides it, and the next turn
/// of the event loop makes it again.
fn close(f: &Fixture) {
    f.shell
        .window()
        .window()
        .dispatch_event(WindowEvent::CloseRequested);
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(1));
}

fn sent_count(f: &Fixture) -> usize {
    f.shell.app().machine().controller.inner.sent.len()
}

fn watched() -> Fixture {
    let f = fixture(Config::default());
    f.shell.watch_windows();
    f
}

#[test]
fn a_closed_window_is_made_again_and_the_old_one_is_dropped() {
    let f = watched();
    let old = f.shell.window().as_weak();

    close(&f);

    assert!(old.upgrade().is_none(), "the old window is still kept");
    assert!(!f.shell.window().window().is_visible());
}

#[test]
fn a_window_made_again_is_closed_and_made_again_too() {
    let f = watched();
    close(&f);
    f.shell.show_window();
    let second = f.shell.window().as_weak();

    close(&f);

    assert!(
        second.upgrade().is_none(),
        "the second window is still kept"
    );
}

#[test]
fn a_close_and_an_open_again_send_nothing() {
    let f = watched();
    let before = sent_count(&f);

    close(&f);
    f.shell.show_window();

    assert_eq!(sent_count(&f), before);
}

#[test]
fn the_window_made_again_shows_the_same_picture() {
    let f = watched();
    let first = f.shell.window().window().take_snapshot().unwrap();

    close(&f);
    f.shell.show_window();

    let again = f.shell.window().window().take_snapshot().unwrap();
    assert_eq!(again.size(), first.size());
    assert!(again.as_bytes() == first.as_bytes(), "the pictures differ");
    assert_eq!(f.shell.window().get_version(), env!("CARGO_PKG_VERSION"));
}

#[test]
fn a_click_in_the_window_made_again_sends() {
    let f = watched();
    close(&f);
    f.shell.show_window();
    let before = sent_count(&f);

    click(&f.shell.window(), Button, "Silent");

    let sent = f.shell.app().machine().controller.inner.sent[before..].to_vec();
    assert!(sent.starts_with(&PerfMode::Silent.commands(&BLADE_18).unwrap()));
    assert_eq!(selected_preset(&f), "Silent");
}

#[test]
fn the_window_made_again_keeps_the_size() {
    let f = watched();
    let size = PhysicalSize::new(900, 1100);
    f.shell.window().window().set_size(size);

    close(&f);
    f.shell.show_window();

    assert_eq!(f.shell.window().window().size(), size);
}

/// A minimised window's place is off screen: the window made again takes
/// neither it nor the size, or it would open where no one sees it.
#[test]
fn a_window_closed_while_minimised_opens_at_its_own_size() {
    let f = watched();
    let window = f.shell.window();
    window.window().set_size(PhysicalSize::new(900, 1100));
    window.window().set_minimized(true);
    drop(window);

    close(&f);

    assert_ne!(
        f.shell.window().window().size(),
        PhysicalSize::new(900, 1100)
    );
}

/// A maximised window's size is the screen's: the window made again opens
/// maximised, not at that size.
#[test]
fn a_window_closed_while_maximised_opens_maximised() {
    let f = watched();
    f.shell.window().window().set_maximized(true);

    close(&f);

    assert!(f.shell.window().window().is_maximized());
}
