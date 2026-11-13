//! The default mic's mute, on the laptop.

use std::time::{Duration, Instant};

use platform::{MicWatch, toggle_mic_mute};

/// The watch's mute once it is `want`, or as it is after 2 s. The notices
/// come on Windows' own threads.
fn wait_for(watch: &mut MicWatch, want: bool) -> Option<bool> {
    let start = Instant::now();
    loop {
        let muted = watch.muted().unwrap();
        if muted == Some(want) || start.elapsed() > Duration::from_secs(2) {
            return muted;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
#[ignore = "needs a mic, and toggles its mute"]
fn the_mic_mute_is_toggled_and_put_back_and_the_watch_follows() {
    let mut watch = MicWatch::new().unwrap();
    let before = watch.muted().unwrap().unwrap();
    eprintln!("mic muted before: {before}");

    let toggled = toggle_mic_mute().unwrap();
    let seen = wait_for(&mut watch, toggled);
    let back = toggle_mic_mute().unwrap();
    let seen_back = wait_for(&mut watch, back);

    assert_eq!(toggled, !before);
    assert_eq!(back, before);
    assert_eq!(seen, Some(toggled), "the change notice");
    assert_eq!(seen_back, Some(before), "the change notice back");
}
