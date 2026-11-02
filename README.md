# &lt;app-name&gt;

> **Name not final.** `<app-name>` is a placeholder until the repo name is chosen.
> "blade-control" is already used by another Blade tool, so it is not an option.

A small, fast replacement for Razer Synapse on one laptop: the
**Razer Blade 18, RZ09-0529**. It is written in Rust and runs on Windows 11.

It is one app: a tray icon with a flyout, plus a one-page settings window.
It talks straight to the laptop's controller over USB HID. There are no
background services, no account and no cloud.

> **Status:** planning. There is no code yet. See [How it is built](#how-it-is-built).

## Why

Synapse is heavy. It runs several services, wants an account, and uses memory
and CPU all the time just to set a few things on the laptop.

This app does only those few things. The targets are:

- About 0% CPU while the window and flyout are closed. Sensors are read only
  while one of them is open.
- Under 15 MB of RAM with just the tray running, and under 50 MB with the
  window open.
- Starts at login, needs no admin rights, and works with Synapse and its
  driver removed.

## Supported hardware

Only one model is supported. Other Razer laptops and devices are out of scope.

| Field | Value |
| --- | --- |
| Model | Razer Blade 18, RZ09-0529 (SKU RZ09-05299ER4) |
| Controller | USB HID, vendor `1532`, product `02C7`, interfaces MI_00 to MI_03 |
| Tested BIOS | 2.01 (2025-08-04) |
| OS | Windows 11 Home 26H2 (build 26300). Mica needs 22H2 or newer |

## Features (v1)

| Feature | Where in the window |
| --- | --- |
| Refresh rate: 60 / 120 / 240 Hz (440 Hz in 1920×1200 mode) | Display |
| Drop to 60 Hz on battery, restore on AC | Display |
| Native display mode: 3840×2400 at 240 Hz or 1920×1200 at 440 Hz (after restart) | Display |
| Presets: Silent, Balanced, Performance | Performance |
| Custom preset: CPU boost, GPU boost, manual fan | Performance |
| GPU mode: Optimus or dGPU only (MUX switch, after restart) | Performance |
| Battery charge limit (for example 80%) | Performance |
| Keyboard lighting on/off | Keyboard and touchpad |
| Static keyboard colour: swatches and a custom picker | Keyboard and touchpad |
| Lid logo light on/off | Keyboard and touchpad |
| Touchpad on/off | Keyboard and touchpad |
| Live readings: CPU/GPU temperature and load, both fans, battery, power draw | Header card |

Native display mode and GPU mode ship only if they work without the Razer
driver. The USB capture (stage 1 of the plan) decides this.

**Not planned:** lighting effects, per-key colour, brightness slider, global
hotkeys, cloud profiles, and other Razer devices.

## UI

The look follows Windows 11 Settings, so the app feels like part of Windows.

- **Light theme**, the Windows accent colour, **Mica** backdrop, Fluent
  controls and Segoe UI Variable.
- **Tray flyout:** a plain list like the Wi-Fi flyout. It has preset, refresh
  rate, lighting, touchpad, "Reset to Balanced" and "Open window".
- **Window:** one scrolling page, like Settings > System > Display, in a normal
  resizable window. No side menu. A header card shows the laptop, the preset,
  a plugged-in or battery pill and the live readings. Below it are three
  groups: Display, Performance, and Keyboard and touchpad.
- Closing the window hides it to the tray. Quit is in the flyout.

The UI toolkit is [Slint](https://slint.dev) with its Fluent style. Mica comes
from `DwmSetWindowAttribute(DWMWA_SYSTEMBACKDROP_TYPE)`. egui cannot look like
Settings, and WebView2 or Tauri would break the memory target.

## How it is built

Work goes in small stages. Each stage is one sitting and one pull request of
about 200 changed lines at most.

### TDD, red to green

1. **Red:** write one small failing test for the next behaviour, and commit it.
2. **Green:** write the least code that makes it pass.
3. **Refactor:** tidy up while all tests stay green.

A stage is done only when `cargo test`, `cargo clippy -- -D warnings` and
`cargo fmt --check` all pass.

Hardware sits behind small traits (`Transport`, `KeyInjector`, `Registry`,
`DisplayApi`, `PowerEvents`, `Sensors`, `Backdrop`, `Clock`). Each has a fake,
so almost every test runs on any Windows machine without a Blade. Expected
bytes come from a USB capture of Synapse on the real laptop, saved in
`tests/fixtures`. Tests that need the real laptop are marked `#[ignore]` and
run by hand:

```bash
cargo test -- --ignored
```

### Crates

| Crate | Job |
| --- | --- |
| `razer-proto` | Builds and parses packets, the checksum and the command table. No I/O |
| `razer-device` | The `Transport` trait, the HID transport, retries on Busy |
| `blade-core` | Presets, Custom, config (TOML in `%APPDATA%`), apply and re-apply after resume, sensor polling |
| `platform` | All Windows FFI. The only crate allowed `unsafe` |
| `ui` | View models (fully unit-tested) and the Slint markup |
| `blade` (exe) | The tray app. Single instance, starts at login |
| `blade-dev` (exe) | Developer tool for sending single commands |

Crate and exe names may change when the repo name is chosen.

### Coding standards

- **YAGNI:** build only the v1 features. No plugins, no "for later" settings.
- **KISS:** the plainest design that passes the test.
- **DRY:** every command is one entry in one command table, sent by one
  generic `send`. No hand-written 90-byte buffers.
- **Small functions:** about 20 lines, one job each.
- **SOLID:** each layer has one job, small traits, logic depends on traits.
- **Invalid states can't be built:** enums and newtypes such as `PerfMode`,
  `FanMode::{Auto, Manual(Rpm)}`, `Rgb` and `RefreshRate`.
- **Errors:** one `thiserror` enum per library crate, `anyhow` only in the
  exes. No `unwrap()` or `expect()` outside tests.
- **Logging:** `tracing` to a rolling file in `%APPDATA%`. No `println!`.
- **Tools:** `cargo fmt`, `cargo clippy -- -D warnings` with `clippy::pedantic`,
  and `#![deny(unsafe_code)]` in every crate except `platform`. CI fails if
  any of these fail.
- **Commits:** small and logical, with short messages.
