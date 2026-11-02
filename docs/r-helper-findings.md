# r-helper findings (stage 0)

Source: [brandonjank/r-helper](https://github.com/brandonjank/r-helper) at
commit `afa868f` (2026-10), cloned to `D:\rust\r-helper`. MIT.
Read on 2026-11-02 for the Blade 18, RZ09-0529, USB `1532:02C7`.

These facts come from r-helper's code and its own test notes. They are leads,
not golden bytes. Golden bytes still come from the stage 1 capture.

## Transport

| Item | Value | Where |
| --- | --- | --- |
| Interface | MI_02, usage page `0x01`, usage `0x02` | `librazer/src/diagnostics.rs` |
| Interface 3 | Usage page `0x59`: rejects the 90-byte report (Windows error 87) | `docs/blade18-validation.md` |
| Report | 90-byte feature report, report ID `0` prepended on send | `librazer/src/device.rs` |
| Init sequence | None needed on this model | `docs/blade18-validation.md` |
| Timing | 1 ms before send, 2 ms before get, 500 ms between retries, 5 tries | `librazer/src/device.rs` |

### Packet layout

| Byte | Field | Value on send |
| --- | --- | --- |
| 0 | status | `0x00` new. Reply: `0x02` ok, `0x05` not supported |
| 1 | transaction ID | Random by default. `0x1F` "standard framing" is opt-in only. `0xFF` for the native lighting matrix (`030A`, `030B`, `0004`) |
| 2–3 | remaining packets | `0x0000` |
| 4 | protocol type | `0x00` |
| 5 | data size | number of args |
| 6 | command class | high byte of the command |
| 7 | command ID | low byte. Getters have bit `0x80` set |
| 8–87 | args | 80 bytes |
| 88 | CRC | XOR of bytes 2 to 87 |
| 89 | reserved | `0x00` |

Note: with a random transaction ID, `Packet::new` leaves the CRC at `0`. The
controller still answers. So this model may not check the transaction ID or
the CRC on the default path. The capture must show what Synapse sends.

The reply must echo the request: same class, command and transaction ID
(`ensure_matches_report` in `packet.rs`). Setters also check that the reply
starts with the same args. A few getters reply with odd values in bytes 2–3;
r-helper allows those per command.

## Command table (02C7)

Getter = setter with bit `0x80` set on the command ID.

| Feature | Setter | Getter | Args | Values |
| --- | --- | --- | --- | --- |
| Preset + fan mode | `0D02` | `0D82` | `[1, zone, mode, fan]`, zones 1 and 2 | Balanced 0, Performance 2, Custom 4, Silent 5. Fan auto 0, manual 1 |
| CPU / GPU boost | `0D07` | `0D87` | `[1, cluster, level]`, CPU 1, GPU 2 | CPU Low 0 to Boost 3. GPU Low 0 to High 2. Only in Custom |
| Fan target | `0D01` | `0D81` | `[0, zone, rpm/100]` | r-helper allows 4000–4500 RPM only |
| Fan actual RPM | — | `0D88` | `[0, zone, 0]` | `rpm/100` |
| Temperatures | — | `0D85` | `[0, 1, 0, 0]` | Two fields. A tracks CPU load, B tracks the NVIDIA GPU |
| Keyboard brightness | `0303` | `0383` | `[1, 5, value]` | 0–255 |
| Static keyboard colour | `0F02` | `0F82` | `[1, 5, 1, 0, 1, 1, R, G, B]` | Firmware reports rate 0 back |
| Logo on/off | `0300` | `0380` | `[1, 4, on]` | Logo is single-colour |
| Logo effect | `0302` | `0382` | `[1, 4, effect]` | Static 0, Breathing 2 |
| Underside light | `0300` / `0303` | `0380` / `0383` | `[1, 38, …]` | On/off and brightness |
| Battery care | `0712` | `0792` | `[value]` | Off `0x50`, on `0xD0` (bit 7 = on, low bits = 80) |
| Charge once / power flags | `070F` | `078F` | `[flags]` | Bit 2 = charge once. Observed base value 8 |
| Native per-key frame | `030B` | — | 70 bytes, row ≤ 6, columns 0–18 | Transaction `0xFF`. 7 rows on this model |
| Controller firmware | — | `0081` | `[0, 0]` | Read 1.6 / 1.7 |
| GPU mode (MUX) | none | `0D89` | `[0, 0]` | Returns `[2, 0]`. Meaning unknown |
| Panel mode (native res) | none | `0D8E` | `[0]` | Returns `[0]`. Meaning unknown |

## Plan features vs r-helper

| Plan feature | r-helper | Status |
| --- | --- | --- |
| Presets Silent, Balanced, Performance | Set and read back, tested | Works (their tests) |
| Custom: CPU and GPU boost | All 12 pairs tested | Works (their tests) |
| Manual fan | Only 4000–4500 RPM, Balanced mode, AC only | Partly. Real fans reached 2900–4300 RPM, not the target |
| Keyboard brightness | Tested round trip | Works (their tests) |
| Static keyboard colour | Tested red, green, blue | Works (their tests) |
| Lid logo on/off | Tested | Works (their tests) |
| Battery charge limit | On/off at 80% only | Partly. Cutoff at 80% not proven. No other limits seen |
| Red touchpad key | Per-key frame exists (`030B`) | Lead only. Key position and how Synapse does it are unknown |
| Fan RPM reading | `0D88`, both zones, plus a third channel | Works (their tests) |
| CPU temperature | `0D85` field A | Lead. Accuracy not proven |
| GPU mode (MUX) | Getter only | Gap. No setter |
| Native display mode | Getter only | Gap. No setter |
| Refresh rate | Windows display API, 60/120/240 Hz | Works, but not over HID |
| Touchpad on/off | Not in r-helper | Gap (Windows side, not HID) |

## What the capture (stage 1) still needs

1. The transaction ID and CRC that Synapse really sends.
2. GPU mode switch, both ways. Check for a HID setter near `0D09` first, then
   a driver call or WMI with Process Monitor.
3. Native display mode switch, both ways. Same checks.
4. Charge limit: does Synapse offer limits other than 80%?
5. Manual fan: the real RPM range Synapse offers.
6. Red touchpad key: the per-key frame and the key's matrix position.
7. Performance preset bytes, to confirm `0D02` mode 2.
8. One short capture each for brightness, colour, logo, Silent, Balanced and
   Custom boost, to confirm r-helper's bytes.

## Notes for our code

- r-helper keeps a strict allow-list of every setter and its args
  (`ensure_blade18_access` in `packet.rs`). Our command table should do the same.
- After a set, r-helper waits 250 ms, reads back and rolls back on mismatch.
- Fan target writes reset if a mode write follows them. Order: read mode, set
  mode, then set target, per zone.
- State reads retry 3 times, 100 ms apart. Around power changes the controller
  gave mixed-zone and zero replies.
- Its own test notes say Smart App Control blocked its build on this model.
