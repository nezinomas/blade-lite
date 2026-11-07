//! The tray's "Start with Windows" item, from what the Run key starts.

use blade_core::Autostart;
use blade_lite::{
    AUTOSTART_LABEL, AUTOSTART_OTHER_LABEL, AUTOSTART_UNKNOWN_LABEL, autostart_item, takeover_note,
};

#[test]
fn on_is_ticked_with_no_warning() {
    let item = autostart_item(&Ok(Autostart::On));

    assert!(item.ticked);
    assert_eq!(item.label, AUTOSTART_LABEL);
    assert_eq!(item.warning, None);
}

#[test]
fn off_is_not_ticked_with_no_warning() {
    let item = autostart_item(&Ok(Autostart::Off));

    assert!(!item.ticked);
    assert_eq!(item.label, AUTOSTART_LABEL);
    assert_eq!(item.warning, None);
}

#[test]
fn another_copy_is_not_ticked_and_is_named() {
    let other = r#""D:\rust\synapse\target\debug\blade-lite.exe" --tray"#;

    let item = autostart_item(&Ok(Autostart::Other(other.to_owned())));

    assert!(!item.ticked);
    assert_eq!(item.label, AUTOSTART_OTHER_LABEL);
    assert!(item.warning.unwrap().contains(other));
}

#[test]
fn an_unread_run_key_is_not_ticked_and_says_why() {
    let item = autostart_item(&Err("access denied".to_owned()));

    assert!(!item.ticked);
    assert_eq!(item.label, AUTOSTART_UNKNOWN_LABEL);
    assert!(item.warning.unwrap().contains("access denied"));
}

#[test]
fn ticking_over_another_copy_names_the_command_it_replaces() {
    let other = Ok(Autostart::Other(r#""D:\old.exe" --tray"#.to_owned()));

    assert!(takeover_note(&other, true).unwrap().contains(r"D:\old.exe"));
    assert_eq!(takeover_note(&other, false), None);
}

#[test]
fn ticking_over_an_unread_run_key_says_so() {
    assert!(takeover_note(&Err("access denied".to_owned()), true).is_some());
}

#[test]
fn ticking_from_off_or_on_needs_no_note() {
    assert_eq!(takeover_note(&Ok(Autostart::Off), true), None);
    assert_eq!(takeover_note(&Ok(Autostart::On), false), None);
}
