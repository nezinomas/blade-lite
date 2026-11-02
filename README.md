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
