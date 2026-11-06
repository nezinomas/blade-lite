//! Autostart at login, over a fake registry.

use std::collections::HashMap;
use std::io;
use std::path::Path;

use blade_core::{AutostartError, RUN_KEY, RUN_VALUE, TRAY_FLAG, autostart_on, set_autostart};
use platform::UserStrings;

const EXE: &str = r"C:\Apps\Blade Lite\blade-lite.exe";
const COMMAND: &str = r#""C:\Apps\Blade Lite\blade-lite.exe" --tray"#;

/// Text values by key and name. A broken registry fails every call.
#[derive(Default)]
struct FakeRegistry {
    values: HashMap<(String, String), String>,
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
        self.values.remove(&(key.to_owned(), name.to_owned()));
        Ok(())
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
    assert!(autostart_on(&registry, exe()).unwrap());
}

#[test]
fn turning_off_removes_the_run_key() {
    let mut registry = FakeRegistry::with_run_value(COMMAND);

    set_autostart(false, &mut registry, exe()).unwrap();

    assert_eq!(registry.run_value(), None);
    assert!(!autostart_on(&registry, exe()).unwrap());
}

#[test]
fn no_run_key_means_off() {
    assert!(!autostart_on(&FakeRegistry::default(), exe()).unwrap());
}

#[test]
fn a_run_key_for_another_exe_is_off_and_turning_on_points_it_here() {
    let mut registry = FakeRegistry::with_run_value(r#""D:\old\blade-lite.exe" --tray"#);
    assert!(!autostart_on(&registry, exe()).unwrap());

    set_autostart(true, &mut registry, exe()).unwrap();

    assert_eq!(registry.run_value(), Some(COMMAND));
}

#[test]
fn the_path_case_does_not_matter() {
    let registry =
        FakeRegistry::with_run_value(&COMMAND.to_uppercase().replace("--TRAY", "--tray"));

    assert!(autostart_on(&registry, exe()).unwrap());
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
        autostart_on(&registry, exe()),
        Err(AutostartError::Registry(_))
    ));
    assert!(matches!(
        set_autostart(true, &mut registry, exe()),
        Err(AutostartError::Registry(_))
    ));
}
