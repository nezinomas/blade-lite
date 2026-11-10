# Stage plan

One stage is one branch. Tick a stage (`[x]`) in the same branch that finishes it.
Done lines move to `docs/PLAN-done.md` at each retro. Ticket numbers count across both files: a new ticket takes the next number after the highest in either.
Each stage's red and green tests and the open questions are in the full plan:
https://claude.ai/artifact/JSzNYRpwZJyywztJtBSk2P

How a stage is built, reviewed and merged: the `stage` skill and the hardware safety rules in `CLAUDE.md`.
Every stage is reviewed before merge by subagents (`.claude/agents/`): both the `informed-reviewer` and the `fresh-reviewer` when it can change what is sent to the laptop, the `informed-reviewer` alone when it can't ("Review tiers" in `CLAUDE.md`).
Checks only a person can do on the laptop wait in "Still to check on the laptop" below until the user confirms them.

## Extras
- [ ] 49 Same popup for touchpad on and off (the physical key needs 47)

## Bug fixes
- [ ] 37 Fans stay loud after Performance to Silent
- [ ] 46 Charge limit: on AC with the battery above 80% and the limit on, check Windows says "not charging". If it still charges, try sending `070F` after the limit as Synapse does
- [ ] 47 Physical touchpad key: it switches the touchpad in the laptop itself, and Windows is not told, so the red key does not follow it. The keyboard sends report `04 DD` on press and `04 00` on release (Synapse capture, steps 23 and 26). Find where the real touchpad state can be read, then read that report (not a keyboard hook) and repaint the key
- [ ] 60 On a weak charge (no charger, but a monitor powers the laptop over USB-C, so it charges slowly), Synapse did not allow Performance, for lack of power. Do the same
- [ ] 62 A send that fails part way (such as the zones sent, then a boost Busy) leaves the controller with part of the new state, but the app keeps the old one until the next resume sends it again. Decide whether to read back or say so in the error
- [ ] 63 The config rename can fail with access denied while another program (such as Defender or the search indexer) holds `config.toml` or its temp file open. The change stays in memory (ticket 57), but the next start sends the old config. Retry only the rename a few times (such as 3 tries, 20 ms apart) on `PermissionDenied`, if the log ever shows it
- [ ] 71 Window tests for the custom colour field (type `#00B7C3`, Enter, check the colour sent) and the Custom sliders (drag CPU boost one step, check the command). Both reviewers
- [ ] 73 Mic key (numpad 0 on the digit layer, mic on the second layer): red while the mic is muted and Num Lock is off, plain with Num Lock on, as ticket 72. Fn + mic today shows the Windows 11 popup "No supported apps in use for mic mute" and the key stays plain (user, 2026-11-09). First find a mute state Windows really keeps (such as the default capture device's mute, read and watched) and check it on the laptop. Both reviewers
- [ ] 75 Temperature warning: a popup when the CPU or GPU stays above a limit (such as 95 °C) for a few minutes, from the readings the app already polls. Once per hot spell, not on each poll
- [ ] 76 Battery health in the header: wear (full charge capacity against design capacity) and cycle count, read from Windows (WMI `BatteryFullChargedCapacity`, `BatteryStaticData`, `BatteryCycleCount`). Read only, no controller command
- [ ] 77 Export and import settings, and "Reset to defaults". Import goes through the same checks as loading `config.toml` (an invalid file changes nothing), then applies as a start does. Both reviewers
- [ ] 82 The 60 Hz on battery is not saved (ticket 81), so Windows may put the saved rate back while still on battery: after a monitor or dock is plugged in, a GPU switch, the lid, or a resume. The app then leaves the high rate, with the rate buttons locked. First check on the laptop which of these does it (note it in `docs/NOTES.md`). If any does, on `Resumed` or a display change while on battery, set 60 Hz again when the panel is back at the rate saved before battery. Both reviewers
- [ ] 83 A crash on battery, then an app start without a Windows restart, leaves 60 Hz on AC: the start reads 60 Hz, so no rate is kept to put back. When the panel is at 60 Hz on battery and the saved rate differs, take the saved rate as the one to put back. Also: the put-back saves the rate it sets, so a rate that was never saved (set by another tool) then overwrites the user's saved rate; decide whether the put-back should keep the saved rate instead. Both reviewers
- [ ] 84 A Narrator set-value on the brightness slider stays shown, unsent, when the window is minimised and restored or brought back with Alt+Tab: ticket 79 puts the slider back only in `show_window` and `toggle_flyout` (ticket 79 review). Put it back on focus too, if it matters. Display only. Both reviewers
- [ ] 88 After a failed start, preset change or power switch the preset is sent again only on the next power event (ticket 87), so Performance or Custom can keep running on battery while the app shows Balanced until then; a failed register of the power events also leaves the start's power source for the whole run. A power event during the 2 s pause after a wake is sent at once, not after the pause (ticket 87 review). Also: a resume that reads a new source (charger pulled in the sleep, or unknown) changes the preset but not the 60 Hz rate, which follows only events; and no test fails a send partway (after the Custom mode write, before the fan target). Decide whether to send again on the next poll, and to only note the source during the pause. Both reviewers

## Code quality
- [x] 89 `clippy::undocumented_unsafe_blocks = "deny"` in the workspace lints: clippy is clean, and an `unsafe` block without a SAFETY note now fails the build (checked by removing one note)
- [x] 90 `expect_used`, `panic` and `indexing_slicing` denied in `razer-proto`, `razer-device` and `blade-core` (tests allowed in `clippy.toml`). Only 5 library sites, not about 100 (the rest were tests): `Rpm` keeps its checked byte, so a too-wide profile range is refused in `new`; `Request::new`, `Response::args` and the key reader use checked slices. Review: a grid with no rows or no columns now sends no frame (it wrapped to a bad frame row in release), and a lost key source is an error
- [x] 91 `proptest` property tests in `razer-proto/tests/props.rs`: checksum, any reply bytes, wrong checksum refused, request round trip (any id, command, 0 to 80 args), too many args refused, any colour text, hex round trip, fan speed and boost limits. All passed first time: no new bug found. Licence entry added
- [ ] 92 CI: `cargo-deny` (licences, known security advisories, duplicate and banned crates) with a `deny.toml`, and a job that builds on the MSRV (`rust-version` in `Cargo.toml`, 1.85; raise it if the dependencies need more)
- [ ] 93 Run `cargo-mutants` on `razer-proto` and `razer-device`: each change to the code that no test catches is a missing test, most of all near a guard. Add the tests, or note why a mutant is harmless. A one-off run first; decide after it whether to run it in CI. Both reviewers

## Still to check on the laptop
Do these in one go, on AC, with "Start with Windows" on and Num Lock off. First note your preset, keyboard colour, lid logo and native mode; the last step puts them back.

1. **Read-only config (57).** Make `%APPDATA%\blade-lite\config.toml` read-only. Press the power-mode key. The popup and the header show the new mode, and the window shows `sent, but not saved`. Make the file writable again.
2. **Config save (61).** Change the preset in the app. `config.toml` has the new preset, and no `config.toml.tmp` is left in that folder.
3. **Sleep, then hibernate.** Set a keyboard colour that is not white. Sleep the laptop and wake it. About 2 s after wake the colour is back (the 2 s wait after wake). Press Fn + numpad Enter: the preset steps (the ticket 45 test sent only Fn, `04 0A`: Num Lock was on, fixed in ticket 59). Do the same with a hibernate.
4. **Restart (44, 18).** Turn the lid logo off. Pick the FHD 440 Hz native mode. Restart. The app starts with Windows, the logo stays off, and the screen starts at FHD 440 Hz.
5. **Put it back.** Set your native mode, keyboard colour, lid logo and preset back as you noted them. Restart once more for the native mode.
6. **App after ticket 70.** Start the app. Open the window and the flyout once, and click one preset. They work as before (ticket 70 changed only types in the shell code).
7. **Busy (55).** Nothing to do now. If the log ever shows a Busy error, check whether the command took effect.
8. **Brightness drag (68).** In the window, drag the keyboard brightness slider slowly and hold it. The percent beside it follows. The keyboard light changes only when you let go. Do the same in the flyout.
9. **Fans on battery (86).** Unplug the charger. In Balanced, run a CPU load (such as a game or a big build) for 2 minutes. When the app shows the CPU above about 65 °C, the fans show RPM (not 0) and you hear them. If they stay at 0 RPM, stop the load at once and note the CPU temperature.

## Publishing on GitHub
- [ ] 39 Release and installer
- [ ] 40 Signing
- [ ] 78 Installer: uninstall closes the app, removes the "Blade Lite" Run value, and asks whether to keep `%APPDATA%\blade-lite` (config and log). Part of 39 or a ticket beside it

## Maybe later (not tickets yet)
- "Open log folder" in the tray menu
- Update check: read GitHub releases about once a week and show a "new version" note; downloads nothing
