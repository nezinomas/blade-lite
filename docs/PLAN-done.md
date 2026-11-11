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
- [x] 49 Same popup for touchpad on and off: on each change of Windows' touchpad value (the physical key, the app's switch or Windows) the popup shows "Touchpad" over "On" or "Off", also while a resume waits (nothing is sent). A state that can't be read shows nothing. Checked by the user 2026-11-10
- [x] 110 The header and flyout say "Plugged in · Charging" or "Plugged in · Not charging" (the charge limit), from `SYSTEM_BATTERY_STATE.Charging` read with the other readings, only on AC; unknown shows "Plugged in". A power event clears it until the next poll. Checked on the laptop 2026-11-10: the limit switch changes it in about 15 s (Windows updates its battery state that slowly), unplugging at once

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
- [x] 37 Fans stay loud after Performance to Silent: not an app bug. Checked 2026-11-10 after a game: Silent drops the fans to about 1600 RPM in 1 min, and after the app sent fans on auto the controller kept them there for 12 min at 44 to 50 °C. The app's lowest manual fan is 1600 RPM, so it can't make them quieter. Custom with a manual fan, then Silent, is still to check (`docs/NOTES.md`)
- [x] 46 Charge limit: on AC with the battery above 80% and the limit on, check Windows says "not charging". If it still charges, try sending `070F` after the limit as Synapse does. Checked 2026-11-10: limit on (set in the BIOS), on AC at 79.5%, Windows read not charging with no app running; the app's off made it charge at about 90 W, to 84%, and the app's on stopped it again. No `070F` needed (`docs/NOTES.md`)
- [x] 47 Physical touchpad key: nothing to build. Windows' `Enabled` value follows the key, so the app's registry watch already repaints the red key (checked by the user 2026-11-10: on with the app running; off seen at app start, painted red). The old note said otherwise, likely from a stale read (`docs/NOTES.md`)
- [x] 60 Weak charge: the controller's charger read `078C` gives `13` for the Razer charger, `04` for a USB-C monitor, `00` for none (Windows says AC and "adequate" for both chargers). On any charger but the profile's full power one, only the battery presets run, as on battery, and the header says "Plugged in · Weak charger". Read at start, after each resume and power event, before a click or key asks for Performance, and on the 1 s and 10 s polls (not in the wake pause or while Razer software runs; full power back only on two reads in a row); a failed read at start or an event counts as weak. Checked by the user: Razer charger pulled with the monitor in gave Balanced, the pill and a grey Performance; the key stepped only Silent and Balanced (with Num Lock off); the charger back brought Performance. A wake on a weak charge not tried (`docs/NOTES.md`)
- [x] 62 A send that fails part way: the lid logo is read back after a failed switch (a switch that took is kept and saved; a failed read hides it), as the charge limit is. Lighting and presets can't be fully read, so their error now says the laptop may run part of the change; the next change, resume or power event sends all of it again
- [x] 63 Config rename retry on access denied: not built. The ticket asked for it only if the log shows it, and the log has no such line (checked 2026-11-10). Moved to "Maybe later"
- [x] 71 Window tests for the custom colour field (click it, type `#00B7C3`, Enter: the lighting with that colour is sent and saved) and the Custom CPU boost slider (a click one step up sends the Custom commands with boost 2 and saves it; a failed send puts the slider back). Both fail when the field's or the slider's callback is cut
- [x] 73 Mic key (numpad 0, row 5 column 16): Fn + mic sends no Razer code, only Win + Alt + K to Windows (its call-mute shortcut). The app reads those keys while Fn is held (no hook) and toggles the default mic's mute, and the key is red while that mute is on and Num Lock is off; the 1 s poll follows a mute set elsewhere. Windows' "No supported apps" popup still shows (the keys still reach Windows). Checked by the user 2026-11-11 (`docs/NOTES.md`)
- [x] 75 Temperature warning: a "Too hot" popup (10 s, and a log line) when the CPU stays at 100 °C or more, or the GPU at 89 °C or more (where each slows itself down), for 3 min. Once per spell; a spell ends 5 °C under the limit or on a wake. With no view open the app reads the CPU temperature (`0D85`) and an awake GPU every 10 s, not in the 2 s wake pause, after a sleep, nor while Razer software found at start or a wake runs. A `0D85` reply must start with `02` (`docs/NOTES.md`)
- [x] 76 Battery health in the header: a line under the power pill, "Battery health 99% · 86 cycles" (full charge against design capacity), read once a minute while a view polls. Read through the battery driver's read-only queries, not WMI (WMI `BatteryStaticData` needs admin); no controller command. No battery, or nothing known, shows no line (`docs/NOTES.md`)
- [x] 77 A "Settings" group in the window: Export… and Import… (the Windows file dialog, on its own thread so polls and power events go on; one at a time) and Reset (Balanced, white keys at 100%, lid logo on, 60 Hz switch off; charge limit, native mode, rate and touchpad left alone; no confirm step). Import refuses a missing, empty, too big (64 KiB) or non-settings file and any value the model refuses (Custom too, whatever the preset), sending nothing; then sends it as a start does and saves it. A file without lighting or logo keeps the current ones. Export and Import checked by the user 2026-11-10; wrong file and Reset in laptop check 15
- [x] 82 60 Hz on battery after a wake: a lid sleep woke the panel at 240 Hz (a Start menu sleep kept 60 Hz). After a wake on battery the app sets 60 Hz again when the panel runs the rate saved before battery; Windows refused the first set, so it tries again on each 1 s poll, up to 10 times, quietly. Checked by the user: 240 Hz, a blink, then 60 Hz. Monitor, dock and GPU switch not tried (`docs/NOTES.md`)
- [x] 83 After a crash on battery the app's start finds the panel at 60 Hz and takes Windows' saved rate (offered at the current resolution, not 60 Hz) to put back on AC, on quit or after a wake. The put-back is no longer saved, so another program's unsaved rate can't overwrite the user's saved one; it is saved only when Windows has saved 60 Hz. Checked by the user: app ended in Task Manager on battery, started again, charger in, 240 Hz back (`docs/NOTES.md`)
- [x] 97 The flyout placement no longer panics on a screen narrower than the flyout (or one that reports width 0 in a display change): `clamp` with min above max panicked in an event-loop timer. Now its left edge stays on the screen, and the flyout and popup sums saturate, so a broken monitor size can't overflow them (code review 2026-11-10)
- [x] 98 Clicks and the power-mode key do nothing during the 2 s pause after a wake (`Shell::user_act`): the controller may not be awake, and the charger may have been pulled in the sleep, so the key could step to Performance on battery. Each click and key press first follows the power events not yet polled, so a wake seen less than 1 s ago holds it too (the `Shell` now owns the power receiver). The resume then fills the windows again. Power events are not held (ticket 88) (code review 2026-11-10)
- [x] 99 `set_native_mode` takes the model and refuses a mode that is not one of its native modes (`NativeModeError::NotOffered`), with nothing sent: every other setter in core checked the model, this one sent any mode byte for the next boot. The UI already offered only the profile's modes (code review 2026-11-10)
- [x] 100 The lid logo switch is set from the app's logo state on each refresh, as the 60 Hz and vapor switches are, so a click that sent nothing (such as when the touchpad registry read fails) no longer leaves it flipped (code review 2026-11-10)
- [x] 111 Withdrawn: it came from a stale registry read in Claude's shell; the Touchpad switch and the red key follow the touchpad (ticket 47)

## Code quality
- [x] 89 `clippy::undocumented_unsafe_blocks = "deny"` in the workspace lints: clippy is clean, and an `unsafe` block without a SAFETY note now fails the build (checked by removing one note)
- [x] 90 `expect_used`, `panic` and `indexing_slicing` denied in `razer-proto`, `razer-device` and `blade-core` (tests allowed in `clippy.toml`). Only 5 library sites, not about 100 (the rest were tests): `Rpm` keeps its checked byte, so a too-wide profile range is refused in `new`; `Request::new`, `Response::args` and the key reader use checked slices. Review: a grid with no rows or no columns now sends no frame (it wrapped to a bad frame row in release), and a lost key source is an error
- [x] 91 `proptest` property tests in `razer-proto/tests/props.rs` (20): checksum covers only bytes 2 to 87; a reply with a valid checksum refuses unknown status and data size above 80; request header bytes and round trip; too many args; colour text (only `#` and 6 hex digits); Custom mode, boost and fan bytes in order; fan speed, boost, brightness and charge limit (50 to 80) ranges; brightness and charge limit replies read only for their LED or percent; any reply args read without a panic. No bug found; breaking the hex, data size or charge mask check fails them. Licence entry added
- [x] 92 CI runs `cargo deny check` (`deny.toml`: permissive licences plus Slint royalty-free, advisories, duplicates warned, crates.io only) and builds on the MSRV, read from `Cargo.toml`. MSRV raised 1.85 to 1.92 (Slint 1.18 needs it); crates marked `publish = false`. No banned crates listed yet. CI also runs weekly, for new advisories
- [x] 93 One `cargo-mutants` run on `razer-proto` and `razer-device`: 22 missed of 230, 12 after the fixes. Added logo reply tests, `feature_buffer` and `feature_report` (the send buffer and the short-read guard) with tests, and tests that the system clock waits and that random IDs spread. Left: 3 hidapi calls `cargo hw` should catch, 7 in the key reader (ticket 96), 2 harmless (`docs/NOTES.md`). Not in CI: 5 minutes a run
- [x] 94 `FanRpm::from_reply` and `PerfMode::from_reply` take the fan or zone asked for and read a reply only when it echoes it and the storage byte is `01`, as `Brightness` does; otherwise `None` ("reply not understood" in the header errors). Unit, core and property tests added
- [x] 101 The preset mode bytes (`PerfModeBytes`), the Custom mode byte (`CustomLimits::mode`), the logo storage byte (`Logo::storage`) and the battery presets (`battery_presets`) are profile data; nothing sent to the Blade 18 changes (the capture tests pass). `ModelProfile::is_valid` is asserted at build time for each profile: distinct mode bytes, battery presets offered and holding Balanced. A preset reply reads only the model's presets
- [x] 104 The firmware version (`FirmwareVersion`, `0x0081`) and the raw LED level read (`Brightness::level_query` and `level_from_reply`, `0x0383`) live in `razer-proto`; `razer-device`, `blade-dev` and the `cargo hw` brightness test use them, and `Brightness::query` and `from_reply` are built on them. The `cargo hw` test still sets levels with its own `0x0303` (not in the ticket)
- [x] 105 Small repeats gone, nothing sent changes: one `VARIABLE_STORAGE` in `razer-proto`; one `colour_frame` (red touchpad key or not) in `blade-core`; `Preset::perf_mode` beside `From<PerfMode>`; one `in_collection` match in discovery; the brightness slider range in one Slint global (`BrightnessRange`), the unused `KeyboardView::brightness_max` removed, and a window test that both sliders run 0 to 100
- [x] 106 `blade_core::reapply_on_resume` removed, with its 4 tests and their clock and power-event fakes: nothing called it, and it sent the saved preset, not the one for the power source. The app resumes through the shell's timer (`RESUME_PAUSE` stays)
- [x] 109 `docs/protocol-findings.md` has a native mode section: `0x0D0E` set, `0x0D8E` read, one mode byte, the ids in the profile. Its source is Synapse's UI code, not a capture (the ticket said capture), and r-helper's read (ticket 99 review)

## Publishing on GitHub
- [x] 96 The key reader loop is `KeySources`, generic over a one-method `ReadTimeout` trait (hidapi and a fake), with 9 unit tests: 0 bytes is `None`, the report is the bytes read, the first collection to send is the only one read then, a read error, a count over the buffer and a lost source are errors, and the wait is capped for hidapi. `cargo mutants` on `hid.rs` now misses only the thin hidapi calls (`docs/NOTES.md`)
