# Synapse capture findings (stage 1)

Captured on 2026-11-02 with USBPcap 1.5.4 on the Blade 18 (RZ09-0529, USB
`1532:02C7`), Windows 11 Home 26H2, Synapse installed. One capture per
Synapse action, made with `capture/Start-SynapseCapture.ps1`.

The golden reports are in `tests/fixtures/<step>.txt`, one line per report:
`>` host to device, `<` reply, then the interface and the 90 bytes in hex.
`capture/decode_usbpcap.py` makes them from the raw files. Raw files are not
committed (they also hold key presses).

## Transport

| Item | Finding |
| --- | --- |
| Interface | Only MI_02 (`wIndex` 2). Same as r-helper |
| Request | `SET_REPORT` feature, 90 bytes. Reply read with `GET_REPORT` |
| Transaction ID (byte 1) | A counter: 0 to 31, then back to 0. Not random, not fixed. Also for the `030B` frame rows (not `0xFF`) |
| CRC (byte 88) | XOR of bytes 2 to 87. Right in all 4964 reports |
| Reply status | Always `0x02` (ok). No busy replies seen |
| `030B` frame rows | Synapse does not read a reply for these |
| Pause before the read | 1 ms is enough: 0 Busy replies in 100 reads. With no pause, 96 of 100 reads come back short (65 bytes). Checked on the laptop on 2026-11-02, firmware 1.6 |

## Commands seen

Args are bytes 8 onwards. Data size is byte 5.

| Feature | Command | Args Synapse sends | Notes |
| --- | --- | --- | --- |
| Preset | `0D02` | `[01, zone, mode, fan]` for zones 1, 2, 3 and 4 | Silent 5, Balanced 0, Performance 2, Custom 4. r-helper sends zones 1 and 2 only |
| Manual fan flag | `0D02` | `[01, zone, 04, 01]` | fan byte 1 in Custom |
| CPU boost | `0D07` | `[01, 01, level]` | Levels 0 to 3 |
| GPU boost | `0D07` | `[01, 02, level]` | Levels 0 to 3 (r-helper said 0 to 2) |
| Fan target | `0D01` | `[01, zone, rpm/100]` | Synapse has a fan curve, not a slider. It sent 1600 to 2300 RPM as the curve followed the temperature. First arg is `01` (r-helper uses `00`) |
| Fan RPM read | `0D88` | `[01, zone, 00]` | Polled for zones 1 and 2, about once a second |
| Charge limit | `0712` | `[D0]` on, `[50]` off | Then `078F` read and `070F [08]`. Only 80% was offered |
| Keyboard brightness | `0303` | `[01, 05, v]` and `[01, 26, v]` | Keyboard and underside together. v = percent × 255 / 100, rounded up: 0→0, 1→3, 10→26, 50→128, 100→255 |
| Static colour | `030A` + `030B` | `030A [05, 00]`, then 7 frame rows | See below. Synapse sends no `0F02` |
| Logo on/off | `0300` | `[00, 04, on]` | Storage byte `00`, not `01` |
| Logo effect | `0302` | `[00, 04, 00]` | Static |
| Touchpad key | `0059` | `[00, 01, x]`, x 0 or 1 | New command. Sent when the touchpad key is pressed. The key turned red with Synapse, but no frame painted it red. Without Synapse it stays unlit, so Synapse caused it, likely through `0059` |
| Other reads | `078C`, `0084`, `0086`, `0D87` | | Sent once when a page opens |

### Lighting frame (`030B`)

Each row: `[FF, row, 00, 12, then 19 × RGB]`, data size `0x3D`. Rows 0 to 6,
columns 0 to 18. Synapse sends all 7 rows again and again while the colour
picker moves, and once a second on the lighting page.

These positions are always black, so they have no key: row 0 col 0, rows 1–2
cols 0 and 14, row 3 cols 0, 13, 14, row 4 cols 0, 2, 13, 14, row 5 cols 0, 4,
7, 8, row 6 col 0.

### Fn keys

Fn + brightness keys made Synapse send `0303` in steps of `0x10` (16 levels).
Synapse followed them. After removing Synapse (stage 2) the Fn brightness
and volume keys still work, so the firmware handles them.

## Not over USB

| Feature | Finding |
| --- | --- |
| GPU mode | Synapse has no GPU mode switch on this model. It only links to the NVIDIA app ("NVIDIA laptop display mode", Advanced Optimus). Drop it from our app |
| Native display mode | Two options: 3840 × 2400 at 240 Hz, or 1920 × 1200 at 440 Hz. It is a HID command after all, see below. "Cancel" sends nothing, so the capture saw nothing |
| Charge limits other than 80% | Synapse did not offer them |

### Native display mode, from Synapse's code

Process Monitor showed no driver call. Synapse's own UI code (its cached
JavaScript under `%LOCALAPPDATA%\Razer\RazerAppEngine\User Data`) names it
"Mini LED Panel Resolution":

| Item | Value |
| --- | --- |
| Get | `0D8E`, data size 1 (r-helper's "panel mode" getter; it read `[0]`) |
| Set | `0D0E`, data size 1, args `[mode]` |
| Modes | `0` = UHD (3840 × 2400, 240 Hz), `1` = FHD (1920 × 1200, 440 Hz) |
| Flow | Synapse sends the set, then restarts Windows at once. Cancel sends nothing |

These bytes come from code, not from a capture. Clicking "Restart" during a
capture would give a golden report, but it also switches the panel.

## Still open

1. Display mode: optional golden report (click "Restart" while recording).
2. Red touchpad key: after Synapse is removed (stage 2), check if the key still
   turns red by itself. If not, test whether sending `0059` makes it red.
3. Real manual fan range: Synapse only has a curve. We may keep r-helper's
   limits or test the range with `0D01` ourselves.
