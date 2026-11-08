# Stage plan

One stage is one branch. Tick a stage (`[x]`) in the same branch that finishes it.
Each stage's red and green tests and the open questions are in the full plan:
https://claude.ai/artifact/JSzNYRpwZJyywztJtBSk2P

How a stage is built, reviewed and merged: the `stage` skill and the hardware safety rules in `CLAUDE.md`.
Every stage is reviewed before merge by subagents (`.claude/agents/`): both the `informed-reviewer` and the `fresh-reviewer` when it can change what is sent to the laptop, the `informed-reviewer` alone when it can't ("Review tiers" in `CLAUDE.md`).
Checks only a person can do on the laptop wait in "Still to check on the laptop" below until the user confirms them.

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
- [ ] 49 Same popup for touchpad on and off (the physical key needs 47)
- [x] 52 Vapor-chamber light switch (LED `0x26`, off stays off after a brightness change and a hibernate; checked on the laptop)

## Bug fixes
- [x] 36 Only Silent or Balanced on battery
- [ ] 37 Fans stay loud after Performance to Silent
- [x] 38 Keyboard colour input needs guards (only `#RRGGBB` sends a colour)
- [x] 41 `cargo hw` takes the one-copy lock, so it can't run beside the app
- [x] 42 A log file (`%APPDATA%\blade-lite\blade-lite.log`, 5 files of 1 MB, panics too)
- [x] 43 Autostart tick re-read when the menu opens; says "(set to another program)" when the Run key names another exe
- [x] 44 Lid logo comes back on after a sleep (now saved and sent at start and after each resume; checked after a sleep and a hibernate)
- [x] 45 Each reply must echo its request's transaction ID and command, or `Device::send` returns `WrongReply` and does not send again
- [ ] 46 Charge limit: on AC with the battery above 80% and the limit on, check Windows says "not charging". If it still charges, try sending `070F` after the limit as Synapse does
- [ ] 47 Physical touchpad key: it switches the touchpad in the laptop itself, and Windows is not told, so the red key does not follow it. The keyboard sends report `04 DD` on press and `04 00` on release (Synapse capture, steps 23 and 26). Find where the real touchpad state can be read, then read that report (not a keyboard hook) and repaint the key
- [x] 50 Log the sensor and fan read errors (each kind once per 10 minutes)
- [x] 51 A reply with an older transaction ID is read again once, never sent again
- [x] 53 A new transaction ID per send, and a new `Device` starts at a random ID
- [x] 54 Num Lock off after a wake from hibernate: a Windows setting, not the app (fixed in the registry, see `docs/NOTES.md`)
- [x] 55 On Busy, read again (50 ms apart, up to 5 reads), never send again
- [x] 56 Plain `DeviceError` text, commands in hex
- [x] 57 A change the laptop took is kept in memory even if the config save fails; the error says `sent, but not saved`
- [ ] 58 Flyout top row: in the user's screenshot, min, max and close buttons sit in the flyout's header and cover "Plugged in" next to "Blade 18 (RZ09-0529)". Find out why (check whether ticket 48's inactive windows caused it) and fix it
- [ ] 59 Power-mode key works only when the numpad is on its second layer (Num Lock off). Make it work as Synapse did. The user offers two ways: Fn + Enter whatever the numpad layer is, or plain numpad Enter while on the second layer
- [ ] 60 On a weak charge (no charger, but a monitor powers the laptop over USB-C, so it charges slowly), Synapse did not allow Performance, for lack of power. Do the same
- [x] 61 `Config::save` writes a temp file, then renames it over the config
- [ ] 62 A send that fails part way (such as the zones sent, then a boost Busy) leaves the controller with part of the new state, but the app keeps the old one until the next resume sends it again. Decide whether to read back or say so in the error
- [ ] 63 The config rename can fail with access denied while another program (such as Defender or the search indexer) holds `config.toml` or its temp file open. The change stays in memory (ticket 57), but the next start sends the old config. Retry only the rename a few times (such as 3 tries, 20 ms apart) on `PermissionDenied`, if the log ever shows it
- [ ] 64 `cargo test` prints about 600 lines from `crates/blade-lite/tests/report.rs`: `report()` writes to stderr with `writeln!(io::stderr())`, which the test harness does not capture. That costs tokens on every test run. Let a test `Reporter` skip stderr, or capture it, without changing what the app writes
- [ ] 65 Some `cargo hw` tests in `crates/blade-core/tests/hardware.rs` (native mode, lid logo, touchpad) unwrap the "set the other state" step before they put the state back. If that step fails after the controller took it (such as `Busy`), the test panics and leaves the laptop changed: the next boot can start in the other native mode. Keep each `Result`, put the state back, then assert, as the charge-limit and refresh-rate tests do
- [ ] 66 `crates/blade-lite/src/app.rs` (about 500 lines) and `shell.rs` (about 470) are edited in most tickets. Split each by job, so a ticket reads less. Move code only: no change in behaviour

## Still to check on the laptop
Do these in one go, on AC, with "Start with Windows" on and Num Lock off (the power-mode key needs it, ticket 59). First note your preset, keyboard colour, lid logo and native mode; the last step puts them back.

1. **Read-only config (57).** Make `%APPDATA%\blade-lite\config.toml` read-only. Press the power-mode key. The popup and the header show the new mode, and the window shows `sent, but not saved`. Make the file writable again.
2. **Config save (61).** Change the preset in the app. `config.toml` has the new preset, and no `config.toml.tmp` is left in that folder.
3. **Sleep, then hibernate.** Set a keyboard colour that is not white. Sleep the laptop and wake it. About 2 s after wake the colour is back (the 2 s wait after wake). Press Fn + numpad Enter: the preset steps (in the ticket 45 test the key sent only Fn, `04 0A`, without `D3`; Num Lock may have been on, ticket 59). Do the same with a hibernate.
4. **Restart (44, 18).** Turn the lid logo off. Pick the FHD 440 Hz native mode. Restart. The app starts with Windows, the logo stays off, and the screen starts at FHD 440 Hz.
5. **Put it back.** Set your native mode, keyboard colour, lid logo and preset back as you noted them. Restart once more for the native mode.
6. **Busy (55).** Nothing to do now. If the log ever shows a Busy error, check whether the command took effect.

## Publishing on GitHub
- [ ] 39 Release and installer
- [ ] 40 Signing
