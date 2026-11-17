//! The Settings group's buttons: export, import and reset.

use std::path::PathBuf;

use blade_core::{Config, Preset};
use i_slint_backend_testing::AccessibleRole::Button;
use i_slint_backend_testing::ElementHandle;
use platform::FileDialog;
use razer_proto::{BLADE_18, PerfMode};
use slint::ComponentHandle;
use slint::platform::Key;
use tempfile::TempDir;

use super::clicks::resuming;
use super::{
    FAKE_DIALOGS, FAKE_HELD, FAKE_HOLD, FAKE_PICKED, FakePick, Fixture, click, control, fixture,
    press_key, selected_preset, shift_tab,
};

/// The user picks `name` in a folder of its own, which lasts as long as
/// the `TempDir`.
fn pick(name: &str) -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(name);
    FAKE_PICKED.set(FakePick::File(path.clone()));
    (dir, path)
}

/// The header of the group `name`, which opens and closes it.
fn header(f: &Fixture, name: &str) -> ElementHandle {
    control(f.shell.window(), Button, name)
}

fn is_open(f: &Fixture, name: &str) -> bool {
    let state = header(f, name).accessible_description().unwrap();
    assert!(state == "Open" || state == "Closed", "{state}");
    // Ticket 232: the toggle state Narrator reads agrees with the description.
    assert_eq!(header(f, name).accessible_checked(), Some(state == "Open"));
    state == "Open"
}

/// Opens the Settings group if it is closed, then clicks its `label` button.
fn press(f: &Fixture, label: &str) {
    if !is_open(f, "Settings") {
        click(f.shell.window(), Button, "Settings");
    }
    click(f.shell.window(), Button, label);
}

fn shown(f: &Fixture, label: &str) -> bool {
    ElementHandle::find_by_accessible_label(f.shell.window(), label)
        .next()
        .is_some()
}

fn sent_count(f: &Fixture) -> usize {
    f.shell.app().machine().controller.inner.sent.len()
}

#[test]
fn export_writes_the_settings_to_the_picked_file() {
    let saved = Config {
        preset: Preset::Silent,
        ..Config::default()
    };
    let f = fixture(saved);
    let (_dir, path) = pick("out.toml");

    press(&f, "Export…");

    assert_eq!(FAKE_DIALOGS.take(), [FileDialog::Save]);
    assert_eq!(Config::import(&path).unwrap(), saved);
}

#[test]
fn import_sends_saves_and_shows_the_picked_file() {
    let f = fixture(Config::default());
    let (_dir, path) = pick("in.toml");
    let file = Config {
        preset: Preset::Silent,
        ..Config::default()
    };
    file.save(&path).unwrap();
    let before = sent_count(&f);

    press(&f, "Import…");

    assert_eq!(FAKE_DIALOGS.take(), [FileDialog::Open]);
    let sent = f.shell.app().machine().controller.inner.sent[before..].to_vec();
    assert!(sent.starts_with(&PerfMode::Silent.commands(&BLADE_18).unwrap()));
    assert_eq!(f.shell.app().config().preset, Preset::Silent);
    assert_eq!(selected_preset(&f), "Silent");
    assert_eq!(f.shell.popup().get_mode(), "Silent");
}

#[test]
fn a_cancelled_dialog_changes_nothing() {
    let f = fixture(Config::default());
    FAKE_PICKED.set(FakePick::Cancel);
    let before = sent_count(&f);

    press(&f, "Import…");

    assert_eq!(sent_count(&f), before);
    assert_eq!(f.shell.window().get_error(), "");
}

#[test]
fn a_failed_dialog_shows_its_error() {
    let f = fixture(Config::default());
    FAKE_PICKED.set(FakePick::Fail);

    press(&f, "Export…");

    assert!(
        f.shell
            .window()
            .get_error()
            .starts_with("the file dialog failed")
    );
}

#[test]
fn a_broken_import_shows_its_error_and_sends_nothing() {
    let f = fixture(Config::default());
    let (_dir, path) = pick("broken.toml");
    std::fs::write(&path, "preset = \"turbo\"\n").unwrap();
    let before = sent_count(&f);

    press(&f, "Import…");

    assert_eq!(sent_count(&f), before);
    assert!(
        f.shell
            .window()
            .get_error()
            .starts_with("config file is not valid")
    );
}

#[test]
fn reset_sends_and_shows_the_defaults() {
    let f = fixture(Config {
        preset: Preset::Silent,
        ..Config::default()
    });
    let before = sent_count(&f);

    press(&f, "Reset");

    let sent = f.shell.app().machine().controller.inner.sent[before..].to_vec();
    assert!(sent.starts_with(&PerfMode::Balanced.commands(&BLADE_18).unwrap()));
    assert_eq!(f.shell.app().config().preset, Preset::Balanced);
    assert_eq!(selected_preset(&f), "Balanced");
}

#[test]
fn one_dialog_at_a_time_and_its_answer_comes_later() {
    let f = fixture(Config::default());
    let (_dir, path) = pick("in.toml");
    Config {
        preset: Preset::Silent,
        ..Config::default()
    }
    .save(&path)
    .unwrap();
    FAKE_HOLD.set(true);

    press(&f, "Import…");
    press(&f, "Export…");

    assert_eq!(FAKE_DIALOGS.take(), [FileDialog::Open]);
    assert_eq!(f.shell.app().config().preset, Preset::Balanced);

    FAKE_HELD.take().unwrap()();

    assert_eq!(f.shell.app().config().preset, Preset::Silent);
    press(&f, "Export…");
    assert_eq!(FAKE_DIALOGS.take(), [FileDialog::Save]);
}

#[test]
fn a_file_picked_while_the_laptop_wakes_says_to_try_again() {
    let f = fixture(Config::default());
    let (_dir, path) = pick("in.toml");
    Config {
        preset: Preset::Silent,
        ..Config::default()
    }
    .save(&path)
    .unwrap();
    FAKE_HOLD.set(true);
    press(&f, "Import…");
    f.shell.app().machine_mut().controller.inner.sent.clear();
    resuming(&f);

    FAKE_HELD.take().unwrap()();

    assert_eq!(f.shell.app().config().preset, Preset::Balanced);
    assert!(f.shell.window().get_error().contains("try again"));
}

/// Slint's royalty-free licence asks for its `AboutSlint` widget in an
/// About screen (ticket 95).
#[test]
fn the_window_shows_the_slint_credit() {
    let f = fixture(Config::default());

    click(f.shell.window(), Button, "About");

    assert!(shown(&f, "#MadeWithSlint"));
}

/// Ticket 249: About shows the crate's version, the one the installer
/// takes from the exe, and the credit.
#[test]
fn about_shows_the_version_and_the_credit() {
    let f = fixture(Config::default());

    click(f.shell.window(), Button, "About");

    assert!(shown(
        &f,
        &format!("Blade Lite {}", env!("CARGO_PKG_VERSION"))
    ));
    assert!(shown(&f, "Vibe coded with Claude Code"));
}

/// Ticket 227: the Settings and About groups start closed.
#[test]
fn the_settings_and_about_groups_start_closed() {
    let f = fixture(Config::default());

    assert!(!is_open(&f, "Settings"));
    assert!(!is_open(&f, "About"));
    assert!(!shown(&f, "Export…"));
    assert!(!shown(&f, "#MadeWithSlint"));
}

#[test]
fn a_click_on_a_group_header_opens_and_closes_it() {
    let f = fixture(Config::default());

    click(f.shell.window(), Button, "Settings");
    assert!(is_open(&f, "Settings"));
    assert!(shown(&f, "Export…"));
    assert!(!is_open(&f, "About"), "only the clicked group opens");

    click(f.shell.window(), Button, "Settings");
    assert!(!is_open(&f, "Settings"));
    assert!(!shown(&f, "Export…"));

    click(f.shell.window(), Button, "About");
    assert!(is_open(&f, "About"));
    assert!(shown(&f, "#MadeWithSlint"));
    click(f.shell.window(), Button, "About");
    assert!(!shown(&f, "#MadeWithSlint"));
}

/// Narrator's invoke opens and closes a group, as a click does.
#[test]
fn narrator_opens_and_closes_a_group() {
    let f = fixture(Config::default());

    header(&f, "Settings").invoke_accessible_default_action();
    assert!(is_open(&f, "Settings"));
    assert!(shown(&f, "Export…"));

    header(&f, "Settings").invoke_accessible_default_action();
    assert!(!is_open(&f, "Settings"));
}

/// Ticket 232: the header is checkable and checked while its group is open.
/// AccessKit then gives it the UI Automation toggle pattern: Narrator's
/// invoke toggles it, and a change raises a toggle-state event, which Narrator
/// should say (laptop check). A changed description it did not say.
#[test]
fn a_group_header_tells_narrator_its_state_as_a_toggle() {
    let f = fixture(Config::default());
    let settings = header(&f, "Settings");
    assert_eq!(settings.accessible_checkable(), Some(true));
    assert_eq!(settings.accessible_checked(), Some(false));

    settings.invoke_accessible_default_action();
    assert_eq!(settings.accessible_checked(), Some(true));

    settings.invoke_accessible_default_action();
    assert_eq!(settings.accessible_checked(), Some(false));
}

#[test]
fn the_groups_are_closed_again_when_the_window_opens_again() {
    let f = fixture(Config::default());
    click(f.shell.window(), Button, "Settings");
    click(f.shell.window(), Button, "About");
    f.shell.window().hide().unwrap();

    f.shell.show_window();

    assert!(!is_open(&f, "Settings"));
    assert!(!is_open(&f, "About"));
}

/// The headers take the keyboard focus, and Enter or Space opens and closes
/// them, so Export, Import and Reset can still be reached without a mouse.
/// Shift+Tab from the start goes to the last control: the About header.
#[test]
fn the_keyboard_opens_and_closes_a_group() {
    let f = fixture(Config::default());

    shift_tab(&f);
    press_key(&f, Key::Return);
    assert!(is_open(&f, "About"));
    press_key(&f, Key::Space);
    assert!(!is_open(&f, "About"));

    shift_tab(&f);
    press_key(&f, Key::Space);
    assert!(is_open(&f, "Settings"));
}

/// A second start, or Open window, on a window already open only brings it
/// to the front: the groups the user opened stay open.
#[test]
fn the_groups_stay_open_when_an_open_window_is_shown_again() {
    let f = fixture(Config::default());
    click(f.shell.window(), Button, "Settings");
    click(f.shell.window(), Button, "About");

    f.shell.show_window();

    assert!(is_open(&f, "Settings"));
    assert!(is_open(&f, "About"));
}
