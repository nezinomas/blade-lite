//! The default mic's mute, on the laptop.

use platform::{mic_muted, toggle_mic_mute};

#[test]
#[ignore = "needs a mic, and toggles its mute"]
fn the_mic_mute_is_toggled_and_put_back() {
    let before = mic_muted().unwrap();
    eprintln!("mic muted before: {before}");

    let toggled = toggle_mic_mute().unwrap();
    let back = toggle_mic_mute().unwrap();

    assert_eq!(toggled, !before);
    assert_eq!(back, before);
    assert_eq!(mic_muted().unwrap(), before);
}
