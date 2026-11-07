//! Autostart at login, over a fake registry.

use std::collections::HashMap;
use std::ffi::OsString;
use std::io;
use std::path::Path;

use blade_core::{
    APPROVED_KEY, Autostart, AutostartError, RUN_KEY, RUN_VALUE, SHOWN_COMMAND_CHARS, TRAY_FLAG,
    autostart, set_autostart, starts_in_tray,
};
use platform::{UserBytes, UserStrings};

const EXE: &str = r"C:\Apps\Blade Lite\blade-lite.exe";
const COMMAND: &str = r#""C:\Apps\Blade Lite\blade-lite.exe" --tray"#;
/// Another copy of the app, such as a debug build.
const OTHER: &str = r#""D:\rust\synapse\target\debug\blade-lite.exe" --tray"#;

/// Text and binary values by key and name. A broken registry fails every
/// call.
#[derive(Default)]
struct FakeRegistry {
    values: HashMap<(String, String), String>,
    bytes: HashMap<(String, String), Vec<u8>>,
    broken: bool,
}

impl FakeRegistry {
    fn with_run_value(value: &str) -> Self {
        let mut registry = Self::default();
        registry
            .values
            .insert((RUN_KEY.to_owned(), RUN_VALUE.to_owned()), value.to_owned());
        registry
    }

    /// Task Manager's flag for the app: an odd first byte is disabled.
    fn with_approved(mut self, flag: u8) -> Self {
        self.bytes.insert(
            (APPROVED_KEY.to_owned(), RUN_VALUE.to_owned()),
            vec![flag, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
        );
        self
    }

    fn approved(&self) -> Option<&Vec<u8>> {
        self.bytes
            .get(&(APPROVED_KEY.to_owned(), RUN_VALUE.to_owned()))
    }

    fn run_value(&self) -> Option<&str> {
        self.values
            .get(&(RUN_KEY.to_owned(), RUN_VALUE.to_owned()))
            .map(String::as_str)
    }

    fn check(&self) -> io::Result<()> {
        if self.broken {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        Ok(())
    }
}

impl UserStrings for FakeRegistry {
    fn read_user_string(&self, key: &str, name: &str) -> io::Result<String> {
        self.check()?;
        self.values
            .get(&(key.to_owned(), name.to_owned()))
            .cloned()
            .ok_or(io::ErrorKind::NotFound.into())
    }

    fn write_user_string(&mut self, key: &str, name: &str, value: &str) -> io::Result<()> {
        self.check()?;
        self.values
            .insert((key.to_owned(), name.to_owned()), value.to_owned());
        Ok(())
    }

    fn delete_user_value(&mut self, key: &str, name: &str) -> io::Result<()> {
        self.check()?;
        let id = (key.to_owned(), name.to_owned());
        self.values.remove(&id);
        self.bytes.remove(&id);
        Ok(())
    }
}

impl UserBytes for FakeRegistry {
    fn read_user_bytes(&self, key: &str, name: &str) -> io::Result<Vec<u8>> {
        self.check()?;
        self.bytes
            .get(&(key.to_owned(), name.to_owned()))
            .cloned()
            .ok_or(io::ErrorKind::NotFound.into())
    }
}

fn exe() -> &'static Path {
    Path::new(EXE)
}

#[test]
fn turning_on_writes_the_run_key_to_start_in_the_tray() {
    let mut registry = FakeRegistry::default();

    set_autostart(true, &mut registry, exe()).unwrap();

    assert_eq!(registry.run_value(), Some(COMMAND));
    assert!(COMMAND.ends_with(TRAY_FLAG));
    assert_eq!(autostart(&registry, exe()).unwrap(), Autostart::On);
}

#[test]
fn turning_off_removes_the_run_key() {
    let mut registry = FakeRegistry::with_run_value(COMMAND);

    set_autostart(false, &mut registry, exe()).unwrap();

    assert_eq!(registry.run_value(), None);
    assert_eq!(autostart(&registry, exe()).unwrap(), Autostart::Off);
}

#[test]
fn no_run_key_means_off() {
    assert_eq!(
        autostart(&FakeRegistry::default(), exe()).unwrap(),
        Autostart::Off
    );
}

#[test]
fn a_run_key_for_another_exe_names_it_and_turning_on_points_it_here() {
    let mut registry = FakeRegistry::with_run_value(OTHER);
    assert_eq!(
        autostart(&registry, exe()).unwrap(),
        Autostart::Other(OTHER.to_owned())
    );

    set_autostart(true, &mut registry, exe()).unwrap();

    assert_eq!(registry.run_value(), Some(COMMAND));
}

#[test]
fn the_path_case_does_not_matter() {
    let registry =
        FakeRegistry::with_run_value(&COMMAND.to_uppercase().replace("--TRAY", "--tray"));

    assert_eq!(autostart(&registry, exe()).unwrap(), Autostart::On);
}

#[test]
fn a_relative_exe_path_is_refused_and_nothing_is_written() {
    let mut registry = FakeRegistry::default();

    assert!(matches!(
        set_autostart(true, &mut registry, Path::new("blade-lite.exe")),
        Err(AutostartError::NotAbsolute(_))
    ));
    assert_eq!(registry.run_value(), None);
}

#[test]
fn a_broken_registry_is_an_error_not_a_guess() {
    let mut registry = FakeRegistry {
        broken: true,
        ..FakeRegistry::default()
    };

    assert!(matches!(
        autostart(&registry, exe()),
        Err(AutostartError::Registry(_))
    ));
    assert!(matches!(
        set_autostart(true, &mut registry, exe()),
        Err(AutostartError::Registry(_))
    ));
}

#[test]
fn disabled_in_task_manager_is_off() {
    let registry = FakeRegistry::with_run_value(COMMAND).with_approved(0x03);

    assert_eq!(autostart(&registry, exe()).unwrap(), Autostart::Off);
}

#[test]
fn enabled_in_task_manager_is_on() {
    let registry = FakeRegistry::with_run_value(COMMAND).with_approved(0x02);

    assert_eq!(autostart(&registry, exe()).unwrap(), Autostart::On);
}

#[test]
fn an_empty_task_manager_flag_is_off() {
    let mut registry = FakeRegistry::with_run_value(COMMAND);
    registry
        .bytes
        .insert((APPROVED_KEY.to_owned(), RUN_VALUE.to_owned()), Vec::new());

    assert_eq!(autostart(&registry, exe()).unwrap(), Autostart::Off);
}

#[test]
fn turning_on_clears_the_task_manager_flag() {
    let mut registry = FakeRegistry::with_run_value(COMMAND).with_approved(0x03);

    set_autostart(true, &mut registry, exe()).unwrap();

    assert_eq!(registry.approved(), None);
    assert_eq!(autostart(&registry, exe()).unwrap(), Autostart::On);
}

#[test]
fn only_the_exact_tray_flag_starts_in_the_tray() {
    let starts = |args: &[&str]| starts_in_tray(args.iter().map(OsString::from));

    assert!(starts(&["--tray"]));
    assert!(starts(&["--other", "--tray"]));
    assert!(!starts(&[]));
    assert!(!starts(&["--TRAY"]));
    assert!(!starts(&["--tray=1"]));
}

#[test]
fn an_argument_that_is_not_unicode_is_no_crash() {
    use std::os::windows::ffi::OsStringExt;
    let broken = OsString::from_wide(&[0xD800]);

    assert!(!starts_in_tray([broken]));
}

#[test]
fn turning_off_clears_the_task_manager_flag_too() {
    let mut registry = FakeRegistry::with_run_value(COMMAND).with_approved(0x02);

    set_autostart(false, &mut registry, exe()).unwrap();

    assert_eq!(registry.approved(), None);
}

#[test]
fn this_exe_without_the_tray_flag_is_another_command() {
    let registry = FakeRegistry::with_run_value(&format!("\"{EXE}\""));

    assert!(matches!(
        autostart(&registry, exe()).unwrap(),
        Autostart::Other(_)
    ));
}

#[test]
fn another_exe_turned_off_in_task_manager_is_just_off() {
    let registry = FakeRegistry::with_run_value(OTHER).with_approved(0x03);

    assert_eq!(autostart(&registry, exe()).unwrap(), Autostart::Off);
}

#[test]
fn a_long_other_command_is_cut_for_showing() {
    let long = format!(r#""D:\{}.exe""#, "x".repeat(10_000));
    let registry = FakeRegistry::with_run_value(&long);

    let Autostart::Other(shown) = autostart(&registry, exe()).unwrap() else {
        panic!("expected another command");
    };
    assert_eq!(shown.chars().count(), SHOWN_COMMAND_CHARS);
    assert!(long.starts_with(&shown));
}

#[test]
fn control_characters_in_another_command_are_not_shown() {
    let registry = FakeRegistry::with_run_value("\"D:\\a.exe\"\r\nfake log line\u{1b}[2J");

    let Autostart::Other(shown) = autostart(&registry, exe()).unwrap() else {
        panic!("expected another command");
    };
    assert!(!shown.chars().any(char::is_control));
}
