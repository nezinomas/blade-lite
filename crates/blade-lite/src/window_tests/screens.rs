//! Pictures of the window, flyout and popup, compared with the saved ones
//! in `tests/screens`. On a change, the test fails and leaves the new
//! picture and a diff (changed pixels red) in `target/screens`. Look at
//! them; if the change is meant, save the new pictures with
//! `BLESS_SCREENS=1 cargo t` (only the value 1 saves).
//!
//! On GitHub Actions the pictures are drawn but not compared: its fonts
//! differ from the laptop's.

use std::fs::{self, File};
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use blade_core::{Config, Preset};
use i_slint_backend_testing::AccessibleRole::Button;
use platform::PowerEvent;
use slint::{ComponentHandle, Rgba8Pixel, SharedPixelBuffer};
use ui::PowerPill;

use super::{click, fixture};

/// A channel may differ by this much, for rounding.
const CHANNEL_SLACK: u8 = 4;

#[test]
fn window() {
    let f = fixture(Config::default());
    check("window", f.shell.window().window());
}

#[test]
fn window_with_custom_sliders() {
    let f = fixture(Config {
        preset: Preset::Custom,
        ..Config::default()
    });
    check("window-custom", f.shell.window().window());
}

#[test]
fn window_on_battery() {
    let f = fixture(Config::default());
    f.shell.act(|app| app.on_power(PowerEvent::OnBattery));
    check("window-battery", f.shell.window().window());
}

#[test]
fn window_with_an_error() {
    let f = fixture(Config::default());
    f.shell.app().machine_mut().controller.fails = true;
    click(f.shell.window(), Button, "Silent");
    check("window-error", f.shell.window().window());
}

#[test]
fn window_with_the_razer_warning() {
    let f = fixture(Config::default());
    f.shell
        .programs()
        .set(&["Razer Synapse Service", "RazerAppEngine.exe"]);
    f.shell.look_for_razer();
    f.shell.refresh(None);
    check("window-razer", f.shell.window().window());
}

#[test]
fn flyout() {
    let f = fixture(Config::default());
    check("flyout", f.shell.flyout().window());
}

/// The longest power pill text fits in the window and the flyout.
#[test]
fn not_charging() {
    let f = fixture(Config::default());
    let label = slint::SharedString::from(PowerPill::NotCharging.label());
    f.shell.window().set_power(label.clone());
    f.shell.flyout().set_power(label);
    check("window-not-charging", f.shell.window().window());
    check("flyout-not-charging", f.shell.flyout().window());
}

#[test]
fn popup() {
    let f = fixture(Config::default());
    let popup = f.shell.popup();
    popup.set_mode("Performance".into());
    popup.set_shown(true);
    popup.show().expect("the popup shows");
    // Past the fade in.
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(300));
    check("popup", popup.window());
}

#[test]
fn touchpad_popup() {
    let f = fixture(Config::default());
    f.shell.app().machine_mut().registry.touchpad = Some(0);
    f.shell.follow_touchpad();
    // The test backend has no window handle to set up, so it is shown
    // here, as in `popup`.
    let popup = f.shell.popup();
    popup.set_shown(true);
    popup.show().expect("the popup shows");
    // Past the fade in.
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(300));
    check("popup-touchpad", popup.window());
}

#[test]
fn heat_popup() {
    let f = fixture(Config::default());
    let start = Instant::now();
    for second in (0..=ui::HEAT_HOLD.as_secs()).step_by(10) {
        let now = start + Duration::from_secs(second);
        // Both at once: the widest text.
        f.shell.watch_heat(Some(105), Some(105), now);
    }
    // Shown here, as in `touchpad_popup`.
    let popup = f.shell.popup();
    popup.set_shown(true);
    popup.show().expect("the popup shows");
    // Past the fade in.
    i_slint_backend_testing::mock_elapsed_time(Duration::from_millis(300));
    check("popup-heat", popup.window());
}

/// Compares a picture of `window` with the saved one called `name`.
fn check(name: &str, window: &slint::Window) {
    let shot = window.take_snapshot().expect("a picture of the window");
    let saved = saved_path(name);
    if env_is("BLESS_SCREENS", "1") {
        write_png(&saved, &shot);
        return;
    }
    if env_is("GITHUB_ACTIONS", "true") {
        return;
    }
    let Some(baseline) = read_png(&saved) else {
        panic!(
            "no saved picture {}: run BLESS_SCREENS=1 cargo t",
            saved.display()
        );
    };
    let changed = changed_pixels(&baseline, &shot);
    if changed.is_empty() {
        return;
    }
    let out = target_dir().join("screens");
    fs::create_dir_all(&out).expect("the screens folder");
    let (new, diff) = (
        out.join(format!("{name}.png")),
        out.join(format!("{name}.diff.png")),
    );
    write_png(&new, &shot);
    write_png(&diff, &diff_picture(&shot, &changed));
    panic!(
        "{name}: {} pixels changed. New picture: {}, diff: {}",
        changed.len(),
        new.display(),
        diff.display()
    );
}

fn env_is(name: &str, value: &str) -> bool {
    std::env::var(name).is_ok_and(|set| set == value)
}

/// The indices of the pixels that differ, or every pixel if the sizes do.
fn changed_pixels(
    baseline: &SharedPixelBuffer<Rgba8Pixel>,
    shot: &SharedPixelBuffer<Rgba8Pixel>,
) -> Vec<usize> {
    if baseline.size() != shot.size() {
        return (0..shot.as_slice().len()).collect();
    }
    let near = |a: u8, b: u8| a.abs_diff(b) <= CHANNEL_SLACK;
    baseline
        .as_slice()
        .iter()
        .zip(shot.as_slice())
        .enumerate()
        .filter(|(_, (a, b))| {
            !(near(a.r, b.r) && near(a.g, b.g) && near(a.b, b.b) && near(a.a, b.a))
        })
        .map(|(index, _)| index)
        .collect()
}

/// The new picture, faded, with the changed pixels red.
fn diff_picture(
    shot: &SharedPixelBuffer<Rgba8Pixel>,
    changed: &[usize],
) -> SharedPixelBuffer<Rgba8Pixel> {
    let mut diff = shot.clone();
    let pixels = diff.make_mut_slice();
    for pixel in pixels.iter_mut() {
        pixel.a /= 4;
    }
    for &index in changed {
        pixels[index] = Rgba8Pixel::new(0xFF, 0, 0, 0xFF);
    }
    diff
}

fn saved_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/screens")
        .join(format!("{name}.png"))
}

fn target_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target")
}

fn read_png(path: &Path) -> Option<SharedPixelBuffer<Rgba8Pixel>> {
    let decoder = png::Decoder::new(std::io::BufReader::new(File::open(path).ok()?));
    let mut reader = decoder.read_info().expect("a PNG");
    let mut bytes = vec![0; reader.output_buffer_size().expect("a PNG size")];
    let info = reader.next_frame(&mut bytes).expect("the PNG's pixels");
    assert_eq!(info.color_type, png::ColorType::Rgba, "{}", path.display());
    Some(SharedPixelBuffer::clone_from_slice(
        &bytes[..info.buffer_size()],
        info.width,
        info.height,
    ))
}

fn write_png(path: &Path, picture: &SharedPixelBuffer<Rgba8Pixel>) {
    let file = File::create(path).expect("the PNG file");
    let mut encoder = png::Encoder::new(BufWriter::new(file), picture.width(), picture.height());
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("the PNG header");
    writer
        .write_image_data(picture.as_bytes())
        .expect("the PNG pixels");
}
