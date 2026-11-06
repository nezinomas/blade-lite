# Project notes

## Laptop facts

- Keyboard firmware is 1.6.
- A 1 ms pause between send and read is needed. Without it, 96 of 100 replies came back broken.
- Fan range is 1600–4500 RPM. Higher speeds are untested.
- Internal screen is 3840×2400 at 60/120/240 Hz.
- An external 4K monitor is attached. Ignore it.
- Touchpad registry value 0 means off.
- `0D85` replies `[2, cpu °C, gpu, 0]` whatever the args. The CPU value is not checked against another sensor.
- The NVIDIA GPU sleeps in D3 when idle. Its power state (`DEVPKEY_Device_PowerData`) can be read without waking it. An NVML read wakes it, and it sleeps again about 1 s later, even with NVML still loaded.
- Mica works in a Slint 1.18 window (stage 22 spike, code in commit 7a330fb). It needs the winit backend with `with_transparent(true)`, `background: transparent` on the Window, then `DwmExtendFrameIntoClientArea(-1)` and `DWMWA_SYSTEMBACKDROP_TYPE = DWMSBT_MAINWINDOW`. Set them after the event loop has run once, since the HWND does not exist before. Without Mica the window is see-through.
- `UISettings` gives the accent colour as RGB 0, 120, 212.
- The accent colour is also in the registry, `Explorer\Accent\AccentColorMenu`, as ABGR (`FFD47800`). The app reads it there, so it needs no WinRT.
- The window (stage 27) is checked by hand with `docs/window-check.md`.
- Silent works on the CPU (checked 2026-11-06, CPU on full load, on AC). On Performance the CPU ran at 176% speed and 100 °C, fans 3200 RPM. After Silent it fell to 94% and 60 °C at once. The fans took about 45 s to slow to 1600 RPM.
- Silent and Balanced do not change the CPU and GPU boost values (`0D87`) or the fan target (`0D81`). They stay as Custom left them. The controller seems to use them only in Custom.
- Windows has its own power mode. On AC it was "Best performance", on battery "Best power efficiency". The app does not change it. A USB capture cannot show if Synapse did.
- One copy runs at a time on the PC, by the mutex `Global\blade-lite`. A copy in another user's session should get "access denied" and end (not checked with two users yet). A second start sets the event `Local\blade-lite-open`, and the first copy opens its window. With `--tray` (the autostart start) the second copy ends without that. Checked on the laptop 2026-11-06.
- Autostart is the value "Blade Lite" in `HKCU\...\CurrentVersion\Run`: the quoted exe path, then `--tray`.
- `blade-dev` and the `cargo hw` tests take the one-copy lock. They refuse to touch the controller while the app runs, so quit it first.
- The lid logo is LED 4. `0300 [00, 04, on]` sets it, as Synapse does. `0380 [00, 04, 00]` reads back the state just set. `0380 [01, 04, 00]` reads another stored value, which the set does not change (checked 2026-11-06). Whether the logo stays off after a reboot or sleep is not checked yet.
- `0D8E` reads back the native mode chosen with `0D0E` at once. The panel stays as it booted.

## Progress

- Stages 0–29 and 36 are done. See `docs/PLAN.md`.
- Retros ran after stages 5 and 14.

## Working style

- The user starts one stage at a time by saying "next".
- Run a retro about every 5 stages, or sooner if the same thing goes wrong twice.

## Lessons

- Run PowerShell scripts with `-ExecutionPolicy Bypass -File`.
- The git hooks need `git config core.hooksPath .githooks` once. They run fmt, clippy and tests before push, because CI only sees master.
- Check `docs/PLAN.md` against what is actually built. Stage 5 found the order did not match.
- Stage branches are never pushed, so there is nothing to delete on GitHub.
- Still unverified on the laptop: the 2 s wait after wake, a real sleep/wake cycle, and a restart into the FHD 440 Hz native mode.
