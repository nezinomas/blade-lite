# Protocol findings from open-source tools

Offline copy of https://claude.ai/artifact/W9Z1Dxcp4Agbxd3r2tEtwX (2026-11-06), for the fresh reviewer.
It compares blade-lite with razer-laptop-control-no-dkms (Rust, Linux) and librazerblade (C++).
It is a snapshot: some proposed tickets below were built later. Check the code, not this file, for what exists now.

## What matches
- 90-byte report, XOR checksum over bytes 2 to 87, status codes 1/2/3/5.
- Set preset `0x0D02 [storage, zone, mode, fan]`, fan byte 0 = auto, 1 = manual. Both projects use the same layout.
- Custom order: mode on each zone with the manual flag, then boosts `0x0D07`, then fan targets `0x0D01` as rpm/100. razer-laptop-control does the same and notes that a mode write after the fan target resets it.
- Boosts only in Custom. Fan speed only with the manual flag set.
- Fan targets are range-checked and must be whole hundreds. Both other projects only clamp, so blade-lite is stricter.

## Differences that are fine
- Storage byte: blade-lite sends 1 (as Synapse does), the others send 0. Synapse is the better source for this model.
- Mode bytes: razer-laptop-control uses 0 Balanced, 1 Gaming, 2 Creator, 3 Silent, 4 Custom. blade-lite uses Silent `0x05` and Performance `0x02`, from the Synapse capture of this laptop (Blade 18 2025, 02C7). The bytes differ between models.
- Zones: the others set the mode on zones 1 and 2 only. blade-lite sets 1 to 4, as Synapse does.
- GPU boost: the others allow 0 to 2. blade-lite allows 0 to 3, because Synapse sent 3 on this model.
- Fan range: razer-laptop-control has 2200 to 5000 RPM for the Blade 18 2023 (PID 02A0). blade-lite uses 1600 to 4500 for this laptop, from captures. The top end is lower, which is the safer side.

## Proposed tickets (2026-11-06)
1. Check that a reply belongs to its request (guard, the important one). `Device::send` should compare the reply's command class/id (bytes 6 and 7) and transaction id with the request, so a stale or out-of-order reply is never read as success, a fan speed or a preset. razer-laptop-control rejects replies whose class, id or remaining-packets differ.
2. Check the zone echo in reply parsers (guard, small). `FanRpm::from_reply` and `PerfMode::from_reply` read `args[2]` without checking that `args[1]` is the fan or zone asked. Pass the expected zone and return None on a mismatch.
3. Battery charge limit: set `0x0712` with 1 arg, bit 7 = limit on, low 7 bits = percent. Read `0x0792`. Validate the percent to a safe range (Synapse offers 50 to 80) and refuse anything else.
4. Lid logo: on `0x0300 [1, 0x04, 0 or 1]`, effect `0x0302 [1, 0x04, 0 static or 2 breathing]`, read `0x0382`. From older models: check against a Synapse capture of this laptop before using them.
5. Name the Timeout status (tidy). Status `0x04` is Timeout in both projects. Today it parses as `UnknownStatus(4)`, which already fails safe.

## Stage 37 (fans stay loud)
No new lead. The other projects leave Custom the same way: by setting the preset with fan byte 0. Neither reads the preset back before writing.

## Sources
- github.com/Razer-Linux/razer-laptop-control-no-dkms: `razer_control_gui/src/daemon/device.rs`, `data/devices/laptops.json`
- github.com/Meetem/librazerblade: `Packets/PacketFactory.cpp`, `Laptop.cpp`
- github.com/openrazer/openrazer: `driver/razerkbd_driver.h` (Blade 18 PIDs: 2023 02A0, 2024 02B8, 2025 02C7, 2026 02E1)
