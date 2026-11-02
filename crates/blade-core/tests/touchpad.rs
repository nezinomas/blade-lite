//! The touchpad switch, over a fake registry and fake keys.

use std::cell::Cell;
use std::io;

use blade_core::{TOUCHPAD_KEY, TOUCHPAD_VALUE, TouchpadError, set_touchpad, touchpad_on};
use platform::{Key, KeyInjector, Registry};

const TOGGLE: [Key; 3] = [Key::Win, Key::Ctrl, Key::F24];

/// The touchpad state as Windows keeps it. `None` means it can't be read.
struct FakeRegistry {
    enabled: Cell<Option<u32>>,
}

impl FakeRegistry {
    fn new(enabled: Option<u32>) -> Self {
        Self {
            enabled: Cell::new(enabled),
        }
    }
}

impl Registry for FakeRegistry {
    fn read_user_dword(&self, key: &str, name: &str) -> io::Result<u32> {
        assert_eq!((key, name), (TOUCHPAD_KEY, TOUCHPAD_VALUE));
        self.enabled.get().ok_or(io::ErrorKind::NotFound.into())
    }
}

/// Records each key press, and flips the touchpad on Win+Ctrl+F24 as
/// Windows does.
struct FakeKeys<'a> {
    registry: &'a FakeRegistry,
    pressed: Vec<Vec<Key>>,
}

impl KeyInjector for FakeKeys<'_> {
    fn press(&mut self, keys: &[Key]) -> io::Result<()> {
        self.pressed.push(keys.to_vec());
        if keys == TOGGLE {
            let flipped = self.registry.enabled.get().map(|on| 1 - on);
            self.registry.enabled.set(flipped);
        }
        Ok(())
    }
}

fn keys(registry: &FakeRegistry) -> FakeKeys<'_> {
    FakeKeys {
        registry,
        pressed: Vec::new(),
    }
}

#[test]
fn the_state_is_read_from_the_registry() {
    assert!(touchpad_on(&FakeRegistry::new(Some(1))).unwrap());
    assert!(!touchpad_on(&FakeRegistry::new(Some(0))).unwrap());
}

#[test]
fn turning_off_sends_the_shortcut_and_the_state_then_reads_off() {
    let registry = FakeRegistry::new(Some(1));
    let mut keys = keys(&registry);

    set_touchpad(false, &registry, &mut keys).unwrap();

    assert_eq!(keys.pressed, [TOGGLE]);
    assert!(!touchpad_on(&registry).unwrap());
}

#[test]
fn turning_on_sends_the_shortcut_and_the_state_then_reads_on() {
    let registry = FakeRegistry::new(Some(0));
    let mut keys = keys(&registry);

    set_touchpad(true, &registry, &mut keys).unwrap();

    assert_eq!(keys.pressed, [TOGGLE]);
    assert!(touchpad_on(&registry).unwrap());
}

#[test]
fn nothing_is_sent_when_the_touchpad_is_already_in_that_state() {
    let registry = FakeRegistry::new(Some(0));
    let mut keys = keys(&registry);

    set_touchpad(false, &registry, &mut keys).unwrap();

    assert!(keys.pressed.is_empty());
    assert!(!touchpad_on(&registry).unwrap());
}

#[test]
fn a_failed_read_is_an_error_and_nothing_is_sent() {
    let registry = FakeRegistry::new(None);
    let mut keys = keys(&registry);

    assert!(matches!(
        touchpad_on(&registry),
        Err(TouchpadError::Read(_))
    ));
    assert!(matches!(
        set_touchpad(false, &registry, &mut keys),
        Err(TouchpadError::Read(_))
    ));
    assert!(keys.pressed.is_empty());
}

#[test]
fn a_value_other_than_0_or_1_is_an_error_not_a_guess() {
    let registry = FakeRegistry::new(Some(2));
    let mut keys = keys(&registry);

    assert!(matches!(
        touchpad_on(&registry),
        Err(TouchpadError::Unknown(2))
    ));
    assert!(matches!(
        set_touchpad(true, &registry, &mut keys),
        Err(TouchpadError::Unknown(2))
    ));
    assert!(keys.pressed.is_empty());
}
