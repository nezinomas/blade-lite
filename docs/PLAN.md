# Stage plan

One stage is one branch. Tick a stage (`[x]`) in the same branch that finishes it.
Each stage's red and green tests and the open questions are in the full plan:
https://claude.ai/artifact/JSzNYRpwZJyywztJtBSk2P

Before a stage merges, review its diff and hunt for bugs. The app drives
real hardware, so add every guard that fits: validate input, clamp to safe
ranges, and fail safe (do nothing rather than guess). List the findings in
the stage report.

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
- [ ] 30 Lid logo
- [ ] 31 Battery charge limit
- 32 GPU mode: dropped (no switch on this model)
- [ ] 33 Red touchpad key
- [ ] 34 Power-mode key (numpad Enter)
- [ ] 35 60 Hz on battery toggle

## Bug fixes
- [x] 36 Only Silent or Balanced on battery
- [ ] 37 Fans stay loud after Performance to Silent
- [ ] 38 Keyboard colour input needs guards

## Publishing on GitHub
- [ ] 39 Release and installer
- [ ] 40 Signing
