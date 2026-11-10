//! The Settings group's buttons: export, import and reset.

use std::path::PathBuf;

use blade_core::{Config, Preset};
use i_slint_backend_testing::AccessibleRole::Button;
use platform::FileDialog;
use razer_proto::{BLADE_18, PerfMode};
use tempfile::TempDir;

use super::clicks::resuming;
use super::{FAKE_DIALOGS, FAKE_HELD, FAKE_HOLD, FAKE_PICKED, FakePick, Fixture, click, fixture};

/// The user picks `name` in a folder of its own, which lasts as long as
/// the `TempDir`.
fn pick(name: &str) -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(name);
    FAKE_PICKED.set(FakePick::File(path.clone()));
    (dir, path)
}

fn sent_count(f: &Fixture) -> usize {
    f.shell.app().machine().controller.sent.len()
}

#[test]
fn export_writes_the_settings_to_the_picked_file() {
    let saved = Config {
        preset: Preset::Silent,
        ..Config::default()
    };
    let f = fixture(saved);
    let (_dir, path) = pick("out.toml");

    click(f.shell.window(), Button, "Export…");

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

    click(f.shell.window(), Button, "Import…");

    assert_eq!(FAKE_DIALOGS.take(), [FileDialog::Open]);
    let sent = f.shell.app().machine().controller.sent[before..].to_vec();
    assert!(sent.starts_with(&PerfMode::Silent.commands(&BLADE_18).unwrap()));
    assert_eq!(f.shell.app().config().preset, Preset::Silent);
    assert_eq!(f.shell.window().get_preset(), "Silent");
    assert_eq!(f.shell.popup().get_mode(), "Silent");
}

#[test]
fn a_cancelled_dialog_changes_nothing() {
    let f = fixture(Config::default());
    FAKE_PICKED.set(FakePick::Cancel);
    let before = sent_count(&f);

    click(f.shell.window(), Button, "Import…");

    assert_eq!(sent_count(&f), before);
    assert_eq!(f.shell.window().get_error(), "");
}

#[test]
fn a_failed_dialog_shows_its_error() {
    let f = fixture(Config::default());
    FAKE_PICKED.set(FakePick::Fail);

    click(f.shell.window(), Button, "Export…");

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

    click(f.shell.window(), Button, "Import…");

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

    click(f.shell.window(), Button, "Reset");

    let sent = f.shell.app().machine().controller.sent[before..].to_vec();
    assert!(sent.starts_with(&PerfMode::Balanced.commands(&BLADE_18).unwrap()));
    assert_eq!(f.shell.app().config().preset, Preset::Balanced);
    assert_eq!(f.shell.window().get_preset(), "Balanced");
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

    click(f.shell.window(), Button, "Import…");
    click(f.shell.window(), Button, "Export…");

    assert_eq!(FAKE_DIALOGS.take(), [FileDialog::Open]);
    assert_eq!(f.shell.app().config().preset, Preset::Balanced);

    FAKE_HELD.take().unwrap()();

    assert_eq!(f.shell.app().config().preset, Preset::Silent);
    click(f.shell.window(), Button, "Export…");
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
    click(f.shell.window(), Button, "Import…");
    f.shell.app().machine_mut().controller.sent.clear();
    resuming(&f);

    FAKE_HELD.take().unwrap()();

    assert_eq!(f.shell.app().config().preset, Preset::Balanced);
    assert!(f.shell.window().get_error().contains("try again"));
}
