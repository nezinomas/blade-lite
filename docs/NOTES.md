# Project notes

## Laptop facts

- Keyboard firmware is 1.6.
- A 1 ms pause between send and read is needed. Without it, 96 of 100 replies came back broken.
- Fan range is 1600–4500 RPM. Higher speeds are untested.
- Internal screen is 3840×2400 at 60/120/240 Hz.
- An external 4K monitor is attached. Ignore it.
- Touchpad registry value 0 means off.

## Progress

- Stages 0–17 are done. See `docs/PLAN.md`.
- Retros ran after stages 5 and 14.

## Working style

- The user starts one stage at a time by saying "next".
- Run a retro about every 5 stages, or sooner if the same thing goes wrong twice.

## Lessons

- Run PowerShell scripts with `-ExecutionPolicy Bypass -File`.
- The git hooks need `git config core.hooksPath .githooks` once. They run fmt, clippy and tests before push, because CI only sees master.
- Check `docs/PLAN.md` against what is actually built. Stage 5 found the order did not match.
- Stage branches are never pushed, so there is nothing to delete on GitHub.
- Still unverified on the laptop: the 2 s wait after wake, a real sleep/wake cycle, and a real unplug (60 Hz on battery).
