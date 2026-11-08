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
- [x] 44 Lid logo off after a reboot and a sleep: check it stays off. If it comes back on, save it in the config and send it at start and resume (it came back on after a sleep; the switch is now saved and sent at start and after each resume, before the lighting. Checked on the laptop: off and on stay after a sleep and a hibernate)
- [x] 45 Each reply must echo its request's transaction id and command, or `Device::send` returns `WrongReply` and does not send again. The `cargo hw` tests that send commands pass with the check
- [ ] 47 Physical touchpad key: it switches the touchpad in the laptop itself, and Windows is not told, so the red key does not follow it. The keyboard sends report `04 DD` on press and `04 00` on release (Synapse capture, steps 23 and 26). Find where the real touchpad state can be read, then read that report (not a keyboard hook) and repaint the key
- [x] 51 If the log shows a `WrongReply` with an older transaction ID (a reply read too soon), pause and read again once before giving up, without sending again (any other transaction ID is read once more per send attempt; a re-read never sends)
- [x] 53 Busy retries send the same transaction ID, so a Busy reply read too soon matches and the command goes out once more. A new `Device` also starts at ID 0, so a reply left from an earlier run can match. Check the Synapse capture: does it keep the ID on a Busy retry? If not, take a new ID per retry (the capture has no Busy reply and never resends a request with its ID, so each retry now takes a new ID; a new `Device` starts at a random ID. `cargo hw` for the controller passed 4 times, each from a random first ID)
- [x] 50 Log the sensor and fan read errors (now shown blank, never logged), and say an error that comes and goes only once in a while, not on each change (each failed Windows or controller read is logged as `read failed: <read>: <error>`, then not again for 10 minutes; details such as a transaction ID don't count as a new error)
- [ ] 46 Charge limit: on AC with the battery above 80% and the limit on, check Windows says "not charging". If it still charges, try sending `070F` after the limit as Synapse does
- [x] 48 Power-mode popup above the taskbar clock: takes no focus, fades out, semi-transparent black like Synapse's (shows the new mode after any change that worked, from the key, the app or the power source; every window now opens without the focus and the window and flyout ask for it. Checked on the laptop: focus stays in Notepad, clicks pass through, flyout closes on an outside click, a second start brings the window up, unplugging Performance says Balanced)
- [ ] 49 Same popup for touchpad on and off (the physical key needs 47)
- [x] 52 Vapor-chamber light switch (LED `0x26`, set with `0303` after the keyboard: the keyboard level while on, 0 while off; saved in the config and sent at start and resume. Checked on the laptop: the switch, and off stays off after a brightness change and a hibernate)
- [x] 54 Num Lock off after a wake from hibernate: Windows, not the app. The lock screen setting `HKU\.DEFAULT\Control Panel\Keyboard\InitialKeyboardIndicators` was the default `0x80000000`; the user set it to `2`. Checked on the laptop: Num Lock now stays as it was before hibernate, on or off
- [x] 55 On a Busy reply, the command is sent again. If Busy means the controller took it and is still working, it runs twice. Check what OpenRazer, razer-laptop-control and librazerblade do on Busy. If they read again without a resend, do the same (up to `MAX_ATTEMPTS` reads) (OpenRazer treats Busy as done, the others mostly ignore it, two forks resend, none reads again. Now a command is sent once; on Busy the reply is read again 50 ms later, up to 5 Busy reads (`MAX_BUSY_READS`), then a Busy error. `cargo hw` for the controller passes)
- [x] 56 Controller errors in the log use Rust debug text: commands show in decimal (`3713`, not `0D81`) and a Windows error is a long `Os { .. }` block. Give `DeviceError` a plain text with commands in hex (each error now has one plain line, such as `the reply is for another request (it echoes transaction ID 4, command 0D81)` or `the HID layer failed: <Windows message> (os error N)`; details that change go in brackets, so the log still says each kind once per 10 minutes)

- [ ] 57 A preset that is sent but whose config save fails: the controller runs the new preset, but the app keeps the old one (header, window, and no power mode popup). Keep the sent preset in memory as the logo switch does, or say in the error that it was sent but not saved
- [ ] 58 Flyout top row: in the user's screenshot, min, max and close buttons sit in the flyout's header and cover "Plugged in" next to "Blade 18 (RZ09-0529)". Find out why (check whether ticket 48's inactive windows caused it) and fix it
- [ ] 59 Power-mode key works only when the numpad is on its second layer (Num Lock off). Make it work as Synapse did. The user offers two ways: Fn + Enter whatever the numpad layer is, or plain numpad Enter while on the second layer
- [ ] 60 On a weak charge (no charger, but a monitor powers the laptop over USB-C, so it charges slowly), Synapse did not allow Performance, for lack of power. Do the same

## Still to check on the laptop
- The power-mode key after a real sleep (in the ticket 45 test the key twice sent only Fn, `04 0A`, without `D3`)
- The 2 s wait after wake, over a real sleep and wake cycle
- A restart into the FHD 440 Hz native mode
- Ticket 44: lid logo off stays off after a restart
- Ticket 55: the read again after a Busy reply has not run on the laptop (no Busy reply seen yet). If the log ever shows a Busy error, check whether the command took effect

## Publishing on GitHub
- [ ] 39 Release and installer
- [ ] 40 Signing
