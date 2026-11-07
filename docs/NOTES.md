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
- With fans on auto, the fans can stop (0 RPM) at idle in every preset: Silent, Balanced, Performance and Custom (checked 2026-11-06, on AC, CPU load 1–12%, about 2 min each). They start at about 1300–1600 RPM when the CPU gets to about 55–65 °C, and stop again when it cools. Balanced stopped and stayed off below about 54 °C. Performance and Custom switched on and off more often, up to 2000 RPM. The controller has no idle speed between 0 and about 1300 RPM.
- A manual Custom fan never stops: the lowest target is 1600 RPM.
- Windows has its own power mode. On AC it was "Best performance", on battery "Best power efficiency". The app does not change it. A USB capture cannot show if Synapse did.
- One copy runs at a time on the PC, by the mutex `Global\blade-lite`. A copy in another user's session should get "access denied" and end (not checked with two users yet). A second start sets the event `Local\blade-lite-open`, and the first copy opens its window. With `--tray` (the autostart start) the second copy ends without that. Checked on the laptop 2026-11-06.
- Autostart is the value "Blade Lite" in `HKCU\...\CurrentVersion\Run`: the quoted exe path, then `--tray`. The tray reads it again on each right click, before the menu opens (in the same call, not posted, so the menu never shows an old tick), and after each click on the item. Only this exe, quoted, then `--tray`, ticks the item; case, spaces and another path to the same file do not matter. A blank value is off. If the value starts another command (such as a copy in `target\debug`), the item says "(set to another program)" and the log names that command once, without control or bidi characters, cut to 200. A click points it at this copy and logs the command it replaced. If the Run key can't be read, the item says "(unknown)" and is not ticked. A `REG_DWORD` value reads as unknown.
- `blade-dev` and the `cargo hw` tests take the one-copy lock. They refuse to touch the controller while the app runs, so quit it first.
- The lid logo is LED 4. `0300 [00, 04, on]` sets it, as Synapse does. `0380 [00, 04, 00]` reads back the state just set. `0380 [01, 04, 00]` reads another stored value, which the set does not change (checked 2026-11-06). The `0380` read is not in the capture: it rests on that check and the `cargo hw` logo test. Each switch also sets the effect to static, as Synapse does. The app reads the state at start and after resume, not on each poll. Whether the logo stays off after a reboot or sleep is not checked yet.
- The charge limit is `0712 [v]`: bit 7 on, low 7 bits the percent. Synapse sends `D0` (on, 80%) or `50` (off), and offers only 80%. `0792 [00]` reads it back with the same layout and the same command id (checked 2026-11-06, it replied `D0`). Synapse then reads `078F` and writes the same flags back with `070F [08]`. The app skips that; the `cargo hw` test checks `078F` stays `08`. The app reads the limit at start and after resume, and reads it back after each set. Whether charging really stops at 80% is not checked yet.
- The touchpad key (numpad ".") is at row 5, column 17 of the key grid (found by eye, 2026-11-07). The app paints it red in the static colour frame while the touchpad is off: at start, after resume, after a lighting change, and each time the touchpad's registry value changes (a `RegNotifyChangeKeyValue` watch, so the physical key counts too). With no saved lighting the app paints nothing. Synapse also sent `0059 [00, 01, x]` on each press, and `00D9 [00, 01, 00]` reads `x` back, but `0059` does not light the key (checked by eye). The app does not send it, since what it does is unknown. `00D9` stays the same when the physical key is pressed.
- The physical touchpad key switches the touchpad in the laptop itself. Windows is not told: its `Enabled` value stays the same, so the red key does not follow that key yet (ticket 47). On a press the keyboard sends input report `04 DD`, then `04 00` on release; Synapse then sent `0059`. Checked 2026-11-07.
- The power-mode key is Fn + numpad Enter. Numpad Enter alone is a plain Enter. The keyboard sends 16-byte input reports on interface 1, usage page 1, usage 0 (two such collections; only one sent them in the probe). Each report is `04`, then the codes of the Razer keys held, then zeros: `0A` is Fn, `D3` the power-mode key, `DD` the touchpad key. Fn + numpad Enter gives `04 0A 00�`, `04 0A D3 00�`, `04 0A 00�`, `04 00�` (read on the laptop 2026-11-07). The keyboard does not change the preset by itself: the `cargo hw` key test reads `0D82` before and after a press. These collections can be read without admin; the keyboard collections can't (access denied).
- The app steps the presets in the profile order Silent, Balanced, Performance, then wraps. Synapse's order is not known (no capture). Custom is never stepped to, since it may hold a manual fan; from Custom the key goes to Silent. On battery it steps Silent and Balanced only, from the preset that runs, and saves the new one, as a click does. A press within 300 ms of the last step is dropped. The reader opens the reports again after a read error or a minute with no report, so a handle gone stale over a sleep comes back.
- The "Switch to 60 Hz on battery" switch is in the window and the flyout. It flips the saved value, so it works while the controller can't be read. On battery it acts at once. If 60 Hz can't be set, it saves the switch off again, so the rate buttons don't lock at another rate. Both switches are set from the config after each click, even after an error.
- The app writes its errors to `%APPDATA%\blade-lite\blade-lite.log`, as well as stderr. Each line starts with the UTC time. A file moves to `.log.1` at 1 MB; 5 files are kept, so the log never passes 5 MB. A window error is written once, not on each refresh. A panic is written too. If the files can't be moved aside (another program holds one open), the log starts again with a note. A line can hold local paths, such as the user folder. On a full disk a line may be cut. Nothing is sent off the PC. Checked with a release build 2026-11-07.
- `0D8E` reads back the native mode chosen with `0D0E` at once. The panel stays as it booted.

## Progress

- Stages 0–31, 33–36, 38, 41, 42 and 43 are done. See `docs/PLAN.md`.
- Retros ran after stages 5 and 14.

## Working style

- The user starts one stage at a time by saying "next".
- Run a retro about every 5 stages, or sooner if the same thing goes wrong twice.

## Lessons

- Run PowerShell scripts with `-ExecutionPolicy Bypass -File`.
- The git hooks need `git config core.hooksPath .githooks` once. They run fmt, clippy and tests before push, because CI only sees master.
- Check `docs/PLAN.md` against what is actually built. Stage 5 found the order did not match.
- Stage branches are never pushed, so there is nothing to delete on GitHub.
- Still unverified on the laptop: the power-mode key after a real sleep, the 2 s wait after wake, a real sleep/wake cycle, and a restart into the FHD 440 Hz native mode.
