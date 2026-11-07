//! Watching a real registry key, made and removed by each test.

use std::process::Command;
use std::thread;
use std::time::Duration;

use platform::RegistryWatch;

/// Long enough for a change to arrive, short enough for a quick test.
const WAIT: Duration = Duration::from_secs(5);
const QUIET: Duration = Duration::from_millis(200);

/// A key under `HKEY_CURRENT_USER` that lives as long as the test.
struct TestKey(String);

impl TestKey {
    fn new(test: &str) -> Self {
        let key = Self(format!(
            r"Software\blade-lite-test\watch-{test}-{}",
            std::process::id()
        ));
        key.set("0");
        key
    }

    fn set(&self, data: &str) {
        let key = format!(r"HKCU\{}", self.0);
        let status = Command::new("reg")
            .args([
                "add",
                &key,
                "/v",
                "Enabled",
                "/t",
                "REG_DWORD",
                "/d",
                data,
                "/f",
            ])
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
fn a_change_to_a_value_is_seen() {
    let key = TestKey::new("change");
    let watch = RegistryWatch::open(&key.0).unwrap();

    key.set("1");

    assert!(watch.wait(WAIT).unwrap());
}

#[test]
fn no_change_times_out() {
    let key = TestKey::new("quiet");
    let watch = RegistryWatch::open(&key.0).unwrap();

    assert!(!watch.wait(QUIET).unwrap());
}

#[test]
fn each_change_is_seen_again_after_the_last() {
    let key = TestKey::new("again");
    let watch = RegistryWatch::open(&key.0).unwrap();

    key.set("1");
    assert!(watch.wait(WAIT).unwrap());
    assert!(!watch.wait(QUIET).unwrap());
    key.set("0");
    assert!(watch.wait(WAIT).unwrap());
}

#[test]
fn the_watch_outlives_the_thread_that_opened_it() {
    let key = TestKey::new("thread");
    let path = key.0.clone();
    let watch = thread::spawn(move || RegistryWatch::open(&path).unwrap())
        .join()
        .unwrap();

    assert!(!watch.wait(QUIET).unwrap());
    key.set("1");

    assert!(watch.wait(WAIT).unwrap());
}

#[test]
fn a_missing_key_cant_be_watched() {
    assert!(RegistryWatch::open(r"Software\blade-lite-test\no-such-key").is_err());
}
