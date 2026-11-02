//! The Windows accent colour, read from the registry.

use std::io;

use blade_lite::accent_colour;
use platform::Registry;
use razer_proto::Rgb;

/// Holds one DWORD, or fails when it has none.
struct FakeRegistry(Option<u32>);

impl Registry for FakeRegistry {
    fn read_user_dword(&self, key: &str, name: &str) -> io::Result<u32> {
        assert_eq!(key, r"Software\Microsoft\Windows\CurrentVersion\Explorer\Accent");
        assert_eq!(name, "AccentColorMenu");
        self.0.ok_or(io::ErrorKind::NotFound.into())
    }
}

#[test]
fn the_dword_is_alpha_blue_green_red() {
    // What the laptop holds for the default blue accent.
    let colour = accent_colour(&FakeRegistry(Some(0xFFD4_7800)));

    assert_eq!(colour, Some(Rgb::new(0, 120, 212)));
}

#[test]
fn a_missing_value_gives_none() {
    assert_eq!(accent_colour(&FakeRegistry(None)), None);
}
