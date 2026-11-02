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
- `0D8E` reads back the native mode chosen with `0D0E` at once. The panel stays as it booted.

## Progress

- Stages 0–21 are done. See `docs/PLAN.md`.
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
