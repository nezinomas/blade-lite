# Done stages and tickets

Moved here from `docs/PLAN.md` at each retro. Ticket numbers count across both files.

## Before code
- [x] 0 Try r-helper and read its 02C7 code
- [x] 1 Capture Synapse USB traffic
- [x] 2 Remove Synapse and its driver

## Protocol, over fakes
- [x] 3 Checksum
- [x] 4 Reply parser
- [x] 5 Packet builder
- [x] 6 Device with retry
- [x] 7 Probe on the real laptop

## Performance and keyboard
- [x] 8 Keyboard brightness (and the model profile)
- [x] 9 Static colour
- [x] 10 Presets
- [x] 11 Custom
- [x] 12 Config
- [x] 13 Apply and re-apply

## Windows features
- [x] 14 Touchpad
- [x] 15 Refresh rates on offer
- [x] 16 Set refresh rate
- [x] 17 60 Hz on battery
- [x] 18 Native display mode

## Sensors
- [x] 19 Controller readings
- [x] 20 Windows readings
- [x] 21 Poll schedule

## UI
- [x] 22 Mica spike
- [x] 23 Header view model
- [x] 24 Display view model
- [x] 25 Performance view model
- [x] 26 Keyboard and touchpad view model
- [x] 27 Window shell
- [x] 28 Tray flyout
- [x] 29 Single instance and autostart

## Extras
- [x] 30 Lid logo
- [x] 31 Battery charge limit
- 32 GPU mode: dropped (no switch on this model)
- [x] 33 Red touchpad key (follows the Windows touchpad switch; the physical key is 47)
- [x] 34 Power-mode key (Fn + numpad Enter steps Silent, Balanced, Performance; read from the keyboard's key report, no hook; checked on the laptop)
- [x] 35 60 Hz on battery toggle (in the Display group and the flyout; a failed 60 Hz set saves the switch off again)
- [x] 48 Power-mode popup above the taskbar clock (no focus, fades out; shows each mode change that worked; checked on the laptop)
- [x] 52 Vapor-chamber light switch (LED `0x26`, off stays off after a brightness change and a hibernate; checked on the laptop)

## Bug fixes
- [x] 36 Only Silent or Balanced on battery
- [x] 38 Keyboard colour input needs guards (only `#RRGGBB` sends a colour)
- [x] 41 `cargo hw` takes the one-copy lock, so it can't run beside the app
- [x] 42 A log file (`%APPDATA%\blade-lite\blade-lite.log`, 5 files of 1 MB, panics too)
- [x] 43 Autostart tick re-read when the menu opens; says "(set to another program)" when the Run key names another exe
- [x] 44 Lid logo comes back on after a sleep (now saved and sent at start and after each resume; checked after a sleep and a hibernate)
- [x] 45 Each reply must echo its request's transaction ID and command, or `Device::send` returns `WrongReply` and does not send again
- [x] 50 Log the sensor and fan read errors (each kind once per 10 minutes)
- [x] 51 A reply with an older transaction ID is read again once, never sent again
- [x] 53 A new transaction ID per send, and a new `Device` starts at a random ID
- [x] 54 Num Lock off after a wake from hibernate: a Windows setting, not the app (fixed in the registry, see `docs/NOTES.md`)
- [x] 55 On Busy, read again (50 ms apart, up to 5 reads), never send again
- [x] 56 Plain `DeviceError` text, commands in hex
- [x] 57 A change the laptop took is kept in memory even if the config save fails; the error says `sent, but not saved`
- [x] 58 Flyout header without min, max and close (winit keeps their styles on a frameless window and Mica's frame showed them; a style hook now takes them out; checked on the laptop)
- [x] 59 Power-mode key with Num Lock on too: the keyboard then sends only Fn and Windows a plain Enter, so while Fn is held (up to 5 s) and Num Lock is on, the reader reads Enter's state (no hook) and an Enter press steps the preset. The Enter still reaches the open app; either Enter key counts (user's pick)
- [x] 61 `Config::save` writes a temp file, then renames it over the config
- [x] 64 Quiet `cargo test`: tests build `Reporter::quiet` (no stderr), the app's reporter is unchanged; about 600 lines down to a few (the app-reporter and touchpad-key tests still print, on purpose)
- [x] 65 The native mode, lid logo and touchpad `cargo hw` tests keep each step's `Result`, put the state back, then assert the put-back first; a touchpad switch not seen in 3 s is waited out before the restore (checked on the laptop)
- [x] 66 Split by job, code moved only: `app.rs` into `app/` (error, views, clicks, events), `shell.rs` into `shell/` (windows, popup, ticks), `main.rs` threads and tray menu out, `tests/app.rs` into `tests/app/` by topic. Other long Rust files each do one job, so they stay
- [x] 67 `window.slint` split: shared parts, the header and one file per group (display, performance, keyboard); markup moved only, bindings and callbacks the same (checked on the laptop)
- [x] 68 Brightness percent (window and flyout) follows the slider while it is dragged; still sent only on release. After a release, a failed send or a change elsewhere, the slider goes back to the laptop's value (Slint now writes the text, Rust's text is gone)
- [x] 69 The charger test and the rate switch test keep each step's `Result`, put the rate back, then assert the put-back first; the charger test takes the one-copy lock and refuses to start at 60 Hz (both passed on the laptop)
- [x] 70 Window tests in `cargo t`: the window, flyout and popup drawn off screen over fakes (Slint's testing backend), 13 scripted clicks that check the exact commands sent, the window and the log, and 6 pictures compared with `crates/blade-lite/tests/screens/`. `CLAUDE.md` and the `stage` skill make threads run them and ask the user only what software can't see
- [x] 72 Touchpad key (numpad ".") red only while the touchpad and Num Lock are both off; Num Lock read each 1 s poll, nothing painted during a resume pause (profile flag `plain_with_num_lock`; checked on the laptop)
- [x] 74 Razer software warning: at start and after resume the app looks for running Razer programs, services and drivers ("razer", "synapse", or an "rz" start), shows a warning in the window while one runs and opens the window once; sends nothing different. The 5 Razer driver updates are hidden with `tools/Block-RazerDriverUpdates.ps1 -Apply` (run by the user 2026-11-10, all 5 hidden). Whether those drivers break blade-lite was not tried
- [x] 79 Showing the window or flyout puts the brightness slider back to the laptop's value and ends a drag the window lost, so a drag cut short or a Narrator set-value is not left shown and a stale mouse-up sends nothing (checked on the laptop)
- [x] 80 Quitting from the tray on battery puts back the rate from before battery, unless the panel left 60 Hz since (changed outside the app); a failure goes to the log (checked on the laptop)
- [x] 81 The 60 Hz on battery is set without saving it (`ChangeDisplaySettingsExW` flags 0), so a sign-out, restart or crash on battery starts at the saved rate; the put-back on AC still saves (checked on the laptop: charger test, and a crash then sign-out came back at 240 Hz)
- [x] 85 Fn + F10 / F11 keyboard brightness: the keys send no code, only Fn; the app reads the level (`0383`) every 250 ms while Fn is held (up to 30 s) and once when Fn comes up (no hook, not in the wake pause), and shows, saves and sends the vapor light at it. Checked on the laptop 2026-11-10
- [x] 86 Fans still after the app was ended in Task Manager on battery (user, 2026-11-10): no app fault found. A kill sends nothing; a repeat on battery read Balanced with the fan byte auto and the fans running (0–1700 RPM at 51 °C). The app sets a fixed fan only in Custom, never below 1600 RPM, so no setting it sends can stop a fan; a start on battery can send a saved Custom fan (ticket 87). Details in notes
- [x] 87 A start reads the power source (`GetSystemPowerStatus`) before it sends the preset, so a start on battery sends Balanced for a saved Performance or Custom (no manual fan on battery); a resume reads it again, since the charger may have been pulled in the sleep. An unknown source or a failed read counts as battery and is logged, until the next power event. After a failed start, preset change or switch, the next power event sends the preset again, even for the same source, and the 60 Hz rate follows the event even when that send fails (checked on the laptop: a start on battery showed Balanced at once, Performance came back on AC)
