//! Reading the real registry, from a key that each test makes and removes.

use std::io;
use std::process::Command;

use platform::{Registry, UserBytes, UserStrings, WindowsRegistry};

/// A key under `HKEY_CURRENT_USER` that lives as long as the test.
struct TestKey(String);

impl TestKey {
    fn new(test: &str) -> Self {
        Self(format!(
            r"Software\blade-lite-test\{test}-{}",
            std::process::id()
        ))
    }

    fn add(&self, name: &str, kind: &str, data: &str) {
        let key = format!(r"HKCU\{}", self.0);
        let status = Command::new("reg")
            .args(["add", &key, "/v", name, "/t", kind, "/d", data, "/f"])
            .output()
            .unwrap()
            .status;
        assert!(status.success());
    }
}

impl Drop for TestKey {
    fn drop(&mut self) {
        let key = format!(r"HKCU\{}", self.0);
        let _ = Command::new("reg").args(["delete", &key, "/f"]).output();
    }
}

#[test]
fn reads_a_dword() {
    let key = TestKey::new("dword");
    key.add("Enabled", "REG_DWORD", "1");

    assert_eq!(
        WindowsRegistry.read_user_dword(&key.0, "Enabled").unwrap(),
        1
    );
}

#[test]
fn a_missing_value_is_not_found() {
    let key = TestKey::new("missing");
    key.add("Other", "REG_DWORD", "1");

    let error = WindowsRegistry
        .read_user_dword(&key.0, "Enabled")
        .unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::NotFound);
}

#[test]
fn a_value_that_is_not_a_dword_is_an_error() {
    let key = TestKey::new("string");
    key.add("Enabled", "REG_SZ", "1");

    assert!(WindowsRegistry.read_user_dword(&key.0, "Enabled").is_err());
}

#[test]
fn a_written_string_reads_back() {
    let key = TestKey::new("string-write");

    WindowsRegistry
        .write_user_string(&key.0, "Command", r#""C:\A B\x.exe" --tray"#)
        .unwrap();

    assert_eq!(
        WindowsRegistry.read_user_string(&key.0, "Command").unwrap(),
        r#""C:\A B\x.exe" --tray"#
    );
}

#[test]
fn a_missing_string_is_not_found() {
    let key = TestKey::new("string-missing");
    key.add("Other", "REG_SZ", "x");

    let error = WindowsRegistry
        .read_user_string(&key.0, "Command")
        .unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::NotFound);
}

#[test]
fn a_value_that_is_not_text_is_an_error() {
    let key = TestKey::new("string-dword");
    key.add("Command", "REG_DWORD", "1");

    assert!(WindowsRegistry.read_user_string(&key.0, "Command").is_err());
}

#[test]
fn a_deleted_value_is_gone_and_deleting_it_again_is_fine() {
    let key = TestKey::new("string-delete");
    key.add("Command", "REG_SZ", "x");

    WindowsRegistry
        .delete_user_value(&key.0, "Command")
        .unwrap();
    WindowsRegistry
        .delete_user_value(&key.0, "Command")
        .unwrap();

    let error = WindowsRegistry
        .read_user_string(&key.0, "Command")
        .unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::NotFound);
}

#[test]
fn reads_binary_bytes() {
    let key = TestKey::new("binary");
    key.add("Flag", "REG_BINARY", "030000000000");

    assert_eq!(
        WindowsRegistry.read_user_bytes(&key.0, "Flag").unwrap(),
        [3, 0, 0, 0, 0, 0]
    );
}

#[test]
fn text_is_not_read_as_bytes() {
    let key = TestKey::new("not-binary");
    key.add("Flag", "REG_SZ", "on");

    assert!(WindowsRegistry.read_user_bytes(&key.0, "Flag").is_err());
}

#[test]
fn text_too_long_is_refused() {
    let key = TestKey::new("too-long");

    let error = WindowsRegistry
        .write_user_string(&key.0, "Long", &"x".repeat(40 * 1024))
        .unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
}
