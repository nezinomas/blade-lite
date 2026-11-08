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

## Bug fixes
- [x] 36 Only Silent or Balanced on battery
- [ ] 37 Fans stay loud after Performance to Silent
- [x] 38 Keyboard colour input needs guards (only `#RRGGBB` sends a colour; bad text sends nothing and the field shows the colour in use)
- [x] 41 `cargo hw` takes the one-copy lock, so it can't run beside the app
- [x] 42 A log file, since a release build shows no error text (`%APPDATA%\blade-lite\blade-lite.log`, 5 files of 1 MB, panics too)
- [x] 43 Autostart tick: re-read it when the menu opens, and warn if the Run key names another exe (such as `target\debug`). The item says "(set to another program)" and the log names it
- [ ] 44 Lid logo off after a reboot and a sleep: check it stays off. If it comes back on, save it in the config and send it at start and resume
- [x] 45 Each reply must echo its request's transaction id and command, or `Device::send` returns `WrongReply` and does not send again. The `cargo hw` tests that send commands pass with the check
- [ ] 47 Physical touchpad key: it switches the touchpad in the laptop itself, and Windows is not told, so the red key does not follow it. The keyboard sends report `04 DD` on press and `04 00` on release (Synapse capture, steps 23 and 26). Find where the real touchpad state can be read, then read that report (not a keyboard hook) and repaint the key
- [x] 51 If the log shows a `WrongReply` with an older transaction ID (a reply read too soon), pause and read again once before giving up, without sending again (any other transaction ID is read once more per send attempt; a re-read never sends)
- [ ] 53 Busy retries send the same transaction ID, so a Busy reply read too soon matches and the command goes out once more. A new `Device` also starts at ID 0, so a reply left from an earlier run can match. Check the Synapse capture: does it keep the ID on a Busy retry? If not, take a new ID per retry
- [ ] 50 Log the sensor and fan read errors (now shown blank, never logged), and say an error that comes and goes only once in a while, not on each change
- [ ] 46 Charge limit: on AC with the battery above 80% and the limit on, check Windows says "not charging". If it still charges, try sending `070F` after the limit as Synapse does
- [ ] 48 Power-mode popup above the taskbar clock: takes no focus, fades out, semi-transparent black like Synapse's
- [ ] 49 Same popup for touchpad on and off (the physical key needs 47)
- [x] 52 Vapor-chamber light switch (LED `0x26`, set with `0303` after the keyboard: the keyboard level while on, 0 while off; saved in the config and sent at start and resume. Switch checked on the laptop)

## Still to check on the laptop
- Vapor light switch off: the light stays off after a brightness change, and after a real sleep and wake (ticket 52)
- The power-mode key after a real sleep (in the ticket 45 test the key twice sent only Fn, `04 0A`, without `D3`)
- The 2 s wait after wake, over a real sleep and wake cycle
- A restart into the FHD 440 Hz native mode

## Publishing on GitHub
- [ ] 39 Release and installer
- [ ] 40 Signing
