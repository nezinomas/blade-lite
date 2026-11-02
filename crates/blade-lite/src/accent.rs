//! The Windows accent colour. The Start menu's copy in the registry matches
//! what `UISettings` gives, and needs no `WinRT`.

use platform::Registry;
use razer_proto::Rgb;

const ACCENT_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\Accent";
const ACCENT_VALUE: &str = "AccentColorMenu";

/// The accent colour, or `None` if Windows does not have one.
pub fn accent_colour(registry: &impl Registry) -> Option<Rgb> {
    let [red, green, blue, _alpha] = registry
        .read_user_dword(ACCENT_KEY, ACCENT_VALUE)
        .ok()?
        .to_le_bytes();
    Some(Rgb::new(red, green, blue))
}
