# blade-lite

A small, fast replacement for Razer Synapse on one laptop: the
**Razer Blade 18, RZ09-0529**. It is written in Rust and runs on Windows 11.

It is one app: a tray icon with a flyout, plus a one-page settings window.
It talks straight to the laptop's controller over USB HID. There are no
background services, no account and no cloud.

> **Status:** works on the author's laptop; some laptop checks are still
> open (see "Still to check on the laptop" in [docs/PLAN.md](docs/PLAN.md));
> no release yet.
> **Use it at your own risk.** It sets the fans and the power limits of an
> expensive laptop. Read [Known limits](#known-limits) and
> [If the fans are stuck](#if-the-fans-are-stuck) first.

## Why

Synapse is heavy. It runs several services, wants an account, and uses memory
and CPU all the time just to set a few things on the laptop.

This app does only those few things. The targets (not measured yet; laptop
check 26 compares a day's use) are:

- About 0% CPU while the window and flyout are closed. With no view open the
  app still reads the CPU and GPU temperature and the charger each 10 s
  (each 2 s while a manual Custom fan runs). These reads can send: Balanced
  on a weak charge, a failed preset again, and the fan guard's full speed.
  A 1 s tick also reads Num Lock, the mic mute and the power source.
- Under 15 MB of RAM with just the tray running, and under 50 MB with the
  window open.
- Starts at login, needs no admin rights, and works with Synapse and its
  driver removed.

## Supported hardware

Only one model is supported. Other Razer laptops and devices are out of scope.

| Field | Value |
| --- | --- |
| Model | Razer Blade 18 2025, RZ09-0529 (SKU RZ09-05299ER4) |
| Controller | USB HID, vendor `1532`, product `02C7` |
| Controller firmware | 1.6 (on this laptop); 1.7 is listed from r-helper, not checked here. Another version, or a failed read, shows a warning; the app still sends |
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
| Presets: Silent, Balanced, Performance; on battery or a weak charge (anything but the Razer charger's reply, such as a USB-C monitor) only Silent and Balanced | Performance |
| Custom preset: CPU boost, GPU boost, manual fan or auto | Performance |
| Battery charge limit (80%) and battery health | Performance |
| Keyboard brightness and a static colour (swatches and a custom colour) | Keyboard and touchpad |
| Lid logo light, vapor chamber light | Keyboard and touchpad |
| Touchpad on/off, with the touchpad key lit to match | Keyboard and touchpad |
| Export, import and reset of the settings | Settings |
| Live readings: CPU and GPU temperature and load, both fans, battery, power draw | Header card |
| The power-mode key steps the preset; a popup shows it | Keyboard |
| Fn + mic mutes the Windows default mic and lights the key | Keyboard |
| Fn + F10 / F11 keyboard brightness is followed by the app | Keyboard |
| Heat warning: a popup when the CPU or GPU stays too hot | Popup |
| Fan guard: a manual Custom fan goes to full speed when the CPU or GPU gets hot | Background |

**Not planned:** lighting effects, per-key colour, global hotkeys, cloud
profiles, GPU mode (no switch on the controller; the NVIDIA app handles it),
and other Razer devices.

## UI

The look follows Windows 11 Settings, so the app feels like part of Windows.

- **Light theme**, the Windows accent colour, **Mica** backdrop, Fluent
  controls and Segoe UI Variable.
- **Tray flyout:** a plain list like the Wi-Fi flyout: preset, refresh rate,
  "Switch to 60 Hz on battery", keyboard brightness, touchpad, "Reset to
  Balanced" and "Open window".
- **Tray menu:** Open window, Start with Windows, and Quit.
- **Window:** one scrolling page, like Settings > System > Display. A header
  card shows the laptop, a plugged-in or battery pill and the live readings.
  Below it are the groups.
- Closing the window hides it to the tray. **Quit** in the tray menu ends the
  app and sends Balanced (Silent stays Silent), with a manual fan set to
  auto first while Custom runs. See the limits below.

The UI toolkit is [Slint](https://slint.dev) with its Fluent style.

## Known limits

- One model only (above). Firmware 1.6 is checked on this laptop; 1.7 is
  listed but not checked here.
- If the app is ended in Task Manager (a kill), it sends nothing: a manual
  Custom fan, the fan guard's full speed or Performance keeps running with
  no app to watch it. A crash during the start is the same (ticket 164).
- Quit on battery or a weak charge sends only Balanced's mode write; whether
  that alone ends a manual fan is not checked (laptop check 12).
- A shutdown, restart or sign-out tries to send the same as Quit; not checked
  on the laptop (laptop check 23), and rarely nothing goes (ticket 159).
- A login start that can't open the controller shows a box once and keeps
  trying every 30 s, with no tray icon until it opens; a start by hand meanwhile
  does nothing (ticket 153).
- While Razer's own software runs, the app warns, and the fan guard and the
  background reads are off, but clicks and the power-mode key still send: a
  manual fan then has no guard. Any program whose name starts with `rz`
  counts. While it sees a Razer updater, it sends nothing, Quit too. The
  updater check goes by name and is a guess (no updater was seen yet).
- The heat warning and the "Fans" popup may not show over a full-screen game
  (ticket 155). The fan guard's GPU part is not checked on the laptop (laptop
  check 19). A GPU whose reads fail never turns the guard on (ticket 163).
- Other Razer chargers and strong USB-C chargers are not checked; they may
  count as weak (only Silent and Balanced).
- Other tools that drive the same controller are not checked beside it:
  r-helper, razer-ctl, OpenRGB, SignalRGB. HWiNFO and MSI Afterburner are not
  checked yet either (laptop check 25). Don't run them at the same time.
- With Num Lock on, the power-mode key (Fn + numpad Enter) also gives Windows
  a plain Enter: the keyboard sends it, not the app. It presses whatever has
  the focus, such as a tile in the Win + A panel or a dialog's default button
  (ticket 198).
- Only one Windows user can run it at a time (not checked with two users;
  tickets 148 and 149).

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
`Registry`, `KeyInjector`, `Sensors` and others). Most have a fake, so almost
every test runs on any Windows machine without a Blade. The window, flyout
and popup are drawn off screen over the fakes (the sensors are still the real
ones, ticket 163), clicked by script, and compared with saved pictures. Tests that need the real laptop
are marked `#[ignore]` and run by hand, one at a time (alias in
`.cargo/config.toml`):

```bash
cargo hw
```

The main checks (CI also runs `cargo deny check`, the capture script tests
and a build on the oldest supported Rust):

```bash
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
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
| [windows](https://crates.io/crates/windows) and [windows-sys](https://crates.io/crates/windows-sys) | Windows APIs (display, power, DWM, registry, devices, audio) |
| [tray-icon](https://crates.io/crates/tray-icon) | The tray icon and its menu |
| [nvml-wrapper](https://crates.io/crates/nvml-wrapper) | GPU temperature and load |
| [serde](https://crates.io/crates/serde) and [toml](https://crates.io/crates/toml) | Config file |
| [thiserror](https://crates.io/crates/thiserror) | Errors |

## Licence

MIT. See [LICENSE](LICENSE).

Razer, Synapse and Blade are trademarks of Razer Inc. This project is not
made by, or connected to, Razer.
