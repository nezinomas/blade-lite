//! Reading the real registry, from a key that each test makes and removes.

use std::io;
use std::process::Command;

use platform::{Registry, WindowsRegistry};

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
