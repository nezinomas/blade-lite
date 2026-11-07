# Stage plan

One stage is one branch. Tick a stage (`[x]`) in the same branch that finishes it.
Each stage's red and green tests and the open questions are in the full plan:
https://claude.ai/artifact/JSzNYRpwZJyywztJtBSk2P

Before a stage merges, review its diff and hunt for bugs. The app drives
real hardware, so add every guard that fits: validate input, clamp to safe
ranges, and fail safe (do nothing rather than guess).

Two separate subagents do the review, not the stage author:
1. Informed reviewer: gets the stage brief, the plan and the repo. It checks
   the diff for bugs and missing guards, and hunts edge cases and paths that
   are unsafe for the hardware.
2. Fresh reviewer: gets only the diff, the repo and the open-source protocol
   findings (https://claude.ai/artifact/W9Z1Dxcp4Agbxd3r2tEtwX). It checks the
   code against them.

Fix what they find, or add it to the plan as a new stage. The stage report
lists each reviewer's findings on its own.

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
- [ ] 35 60 Hz on battery toggle

## Bug fixes
- [x] 36 Only Silent or Balanced on battery
- [ ] 37 Fans stay loud after Performance to Silent
- [ ] 38 Keyboard colour input needs guards
- [x] 41 `cargo hw` takes the one-copy lock, so it can't run beside the app
- [ ] 42 A log file, since a release build shows no error text
- [ ] 43 Autostart tick: re-read it when the menu opens, and warn if the Run key names another exe (such as `target\debug`)
- [ ] 44 Lid logo off after a reboot and a sleep: check it stays off. If it comes back on, save it in the config and send it at start and resume
- [ ] 45 Check that each reply belongs to its request: compare the command id and transaction id in `Device::send`, then run all of `cargo hw`. Some tools accept a `0792` reply whatever its status; this model replied Success
- [ ] 47 Physical touchpad key: it switches the touchpad in the laptop itself, and Windows is not told, so the red key does not follow it. The keyboard sends report `04 DD` on press and `04 00` on release (Synapse capture, steps 23 and 26). Find where the real touchpad state can be read, then read that report (not a keyboard hook) and repaint the key
- [ ] 46 Charge limit: on AC with the battery above 80% and the limit on, check Windows says "not charging". If it still charges, try sending `070F` after the limit as Synapse does

## Publishing on GitHub
- [ ] 39 Release and installer
- [ ] 40 Signing
