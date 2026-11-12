# blade-lite

A small, fast replacement for Razer Synapse on one laptop: the
**Razer Blade 18, RZ09-0529**. It is written in Rust and runs on Windows 11.

It is one app: a tray icon with a flyout, plus a one-page settings window.
It talks straight to the laptop's controller over USB HID. There are no
background services, no account and no cloud.

> **Status:** works on the author's laptop; no release yet.
> **Use it at your own risk.** It sets the fans and the power limits of an
> expensive laptop. Read [Known limits](#known-limits) and
> [If the fans are stuck](#if-the-fans-are-stuck) first.

## Why

Synapse is heavy. It runs several services, wants an account, and uses memory
and CPU all the time just to set a few things on the laptop.

This app does only those few things. The targets are:

- About 0% CPU while the window and flyout are closed. With no view open the
  app still reads the CPU temperature and the charger each 10 s (each 2 s
  while a manual Custom fan runs), for the heat warning and the fan guard.
- Under 15 MB of RAM with just the tray running, and under 50 MB with the
  window open. Not measured yet (laptop check 26).
- Starts at login, needs no admin rights, and works with Synapse and its
  driver removed.

## Supported hardware

Only one model is supported. Other Razer laptops and devices are out of scope.

| Field | Value |
| --- | --- |
| Model | Razer Blade 18 2025, RZ09-0529 (SKU RZ09-05299ER4) |
| Controller | USB HID, vendor `1532`, product `02C7` |
| Controller firmware | 1.6 and 1.7. Another version shows a warning; the app still sends |
| Tested BIOS | 2.01 (2025-08-04) |
| OS | Windows 11 Home 26H2 (build 26300). Mica needs 22H2 or newer |

Everything model-specific is in one model profile
(`crates/razer-proto/src/profile.rs`), so another model would be a new
profile. Only the Blade 18 2025 has one.

## Features

| Feature | Where |
| --- | --- |
| Refresh rate: 60 / 120 / 240 Hz (440 Hz in 1920×1200 mode) | Display |
| Switch to 60 Hz on battery, the old rate back on the charger | Display |
| Native display mode: 3840×2400 at 240 Hz or 1920×1200 at 440 Hz (after a restart) | Display |
| Presets: Silent, Balanced, Performance; on battery or a weak USB-C charge only Silent and Balanced | Performance |
| Custom preset: CPU boost, GPU boost, manual fan or auto | Performance |
| Battery charge limit (80%) and battery health | Performance |
| Keyboard brightness and a static colour (swatches and a custom colour) | Keyboard and touchpad |
| Lid logo light, vapor chamber light | Keyboard and touchpad |
| Touchpad on/off, with the touchpad key lit to match | Keyboard and touchpad |
| Export, import and reset of the settings | Settings |
| Live readings: CPU and GPU temperature and load, both fans, battery, power draw | Header card |
| The power-mode key steps the preset; a popup shows it | Keyboard |
| Heat warning: a popup when the CPU or GPU stays too hot | Popup |
| Fan guard: a manual Custom fan goes to full speed when the CPU or GPU gets hot | Background |

**Not planned:** lighting effects, per-key colour, global hotkeys, cloud
profiles, GPU mode (this model has no switch), and other Razer devices.

## UI

The look follows Windows 11 Settings, so the app feels like part of Windows.

- **Light theme**, the Windows accent colour, **Mica** backdrop, Fluent
  controls and Segoe UI Variable.
- **Tray flyout:** a plain list like the Wi-Fi flyout: preset, refresh rate,
  keyboard brightness, touchpad, "Reset to Balanced" and "Open window".
- **Tray menu:** Open window, Start with Windows, and Quit.
- **Window:** one scrolling page, like Settings > System > Display. A header
  card shows the laptop, a plugged-in or battery pill and the live readings.
  Below it are the groups.
- Closing the window hides it to the tray. **Quit** in the tray menu ends the
  app and sends Balanced (Silent stays Silent), with a manual fan set to
  auto first.

The UI toolkit is [Slint](https://slint.dev) with its Fluent style.

## Known limits

- One model only (above), and controller firmware 1.6 and 1.7 only.
- **Ended in Task Manager** (a kill) the app can send nothing: a manual
  Custom fan, the fan guard's full speed or Performance keeps running with no
  app to watch it. A crash during the start is the same (ticket 164). See
  [If the fans are stuck](#if-the-fans-are-stuck).
- While Razer's own software runs, the app warns, and the fan guard and the
  background reads are off. While it sees a Razer updater, it sends nothing.
  It finds Razer software by name, so it can miss some.
- Other tools that drive the same controller are not checked beside it:
  r-helper, razer-ctl, OpenRGB, SignalRGB. HWiNFO and MSI Afterburner are not
  checked yet either (laptop check 25). Don't run them at the same time.
- Only one Windows user can run it at a time.

## If the fans are stuck

If the fans stay fast or slow, or Performance stays on battery, see
[docs/recovery.md](docs/recovery.md).

## How it is built

Work goes in small stages and tickets, listed in [docs/PLAN.md](docs/PLAN.md).
Each one is built test first (red, green, refactor), then checked by
reviewers before it is merged. Laptop facts are in
[docs/NOTES.md](docs/NOTES.md), and the coding rules in
[CODING_STANDARDS.md](CODING_STANDARDS.md).

Hardware sits behind small traits (`Transport`, `Controller`, `DisplayApi`,
`Registry`, `KeyInjector`, `PowerEvents`, `Sensors` and others). Each has a
fake, so almost every test runs on any Windows machine without a Blade. The
window, flyout and popup are drawn off screen over the fakes, clicked by
script, and compared with saved pictures. Tests that need the real laptop
are marked `#[ignore]` and run by hand, one at a time (alias in
`.cargo/config.toml`):

```bash
cargo hw
```

The checks, as in CI:

```bash
cargo fmt --all --check
```

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

```bash
cargo test --workspace
```

### Crates

| Crate | Job |
| --- | --- |
| `razer-proto` | Builds and parses packets, the checksum, the command table and the model profile. No I/O |
| `razer-device` | The `Transport` trait, the HID transport, reads again on Busy (never sends again), opens the controller again after its handle dies |
| `blade-core` | Presets, Custom, config (TOML in `%APPDATA%\blade-lite`), apply, power and charger rules, sensor polling |
| `platform` | All Windows calls. The only crate allowed `unsafe` |
| `ui` | View models, the heat warning and the fan guard (unit-tested) |
| `blade-lite` (exe) | The tray app, its window and flyout. One copy at a time, starts at login |
| `blade-dev` (exe) | Developer tool for single commands. Not for users |

## Install

There is no release yet. When there is, it will be an installer from the
GitHub Releases page. It needs no admin rights and sets up autostart. Whether
its uninstaller keeps or removes the settings and the log
(`%APPDATA%\blade-lite`) is still to be decided (ticket 78).

### Unsigned releases and SmartScreen

Releases are **not code-signed** for now. Windows SmartScreen may show
"Windows protected your PC" the first time you run the installer.

1. Click **More info**.
2. Click **Run anyway**.

Only do this for files from this repo's Releases page. We plan to apply to
SignPath Foundation (free signing for open source). If accepted, releases will
be signed. winget comes after the first signed release.

### Removing Synapse

The app is made to run with Synapse and its Razer Control Device driver
removed. Step-by-step removal and rollback notes will be added before the
first release. Make a restore point and keep the Synapse installer first.

## Credits

This app ports code and protocol facts from other open-source projects.
Their notices are in [NOTICE](NOTICE), and their full licence texts are in
[THIRD_PARTY_LICENSES.md](THIRD_PARTY_LICENSES.md).

### Reused code

| Project | Licence | What we use |
| --- | --- | --- |
| [r-helper, Blade 18 fork](https://github.com/brandonjank/r-helper) by brandonjank | MIT | The 02C7 support: interface, transaction ID, command table, fan channels, sensor reads and its [capability notes](https://github.com/brandonjank/r-helper/blob/main/docs/razer-controller-capabilities.md). It is a fork of [Fatalution/r-helper](https://github.com/Fatalution/r-helper) (MIT) |
| [razer-ctl](https://github.com/tdakhran/razer-ctl) by tdakhran | MIT | The laptop command set (performance modes, boost, fans, brightness), packet code and reverse-engineering notes |

### Used as documentation only (no code copied)

| Project | Licence | What we learn from it |
| --- | --- | --- |
| [OpenRazer](https://github.com/openrazer/openrazer) | GPL | The 90-byte report format and lighting commands |
| [razer-laptop-control-no-dkms](https://github.com/Razer-Linux/razer-laptop-control-no-dkms) | Not yet checked | A second source for fan and power commands |
| [G-Helper](https://github.com/seerge/g-helper) | GPL-3.0 | The product idea: a small tray app that replaces a heavy vendor suite (ASUS) |

### Dependencies

The main crates the app uses. `cargo deny` (`deny.toml`) checks the licences
of every crate in CI.

| Crate | Use |
| --- | --- |
| [slint](https://slint.dev) | UI toolkit (royalty-free licence for desktop apps, with attribution) |
| [hidapi](https://crates.io/crates/hidapi) | USB HID transport |
| [windows](https://crates.io/crates/windows) and [windows-sys](https://crates.io/crates/windows-sys) | Windows APIs (display, power, DWM, registry, devices) |
| [tray-icon](https://crates.io/crates/tray-icon) | The tray icon and its menu |
| [nvml-wrapper](https://crates.io/crates/nvml-wrapper) | GPU temperature and load |
| [serde](https://crates.io/crates/serde) and [toml](https://crates.io/crates/toml) | Config file |
| [thiserror](https://crates.io/crates/thiserror) | Errors |

## Licence

MIT. See [LICENSE](LICENSE).

Razer, Synapse and Blade are trademarks of Razer Inc. This project is not
made by, or connected to, Razer.
