# Retro: stages 5 to 14 (2026-11-02)

Made with Matt Pocock's `/retro` skill. Source: the 9 stage threads (5 to 14), the repo and git history.
Findings from the first retro (`docs/retro-2026-11-02.md`) that are fixed are not repeated.
Most severe first. Nothing is changed yet. You pick.

## What the last retro fixed (it worked)

- No stage from 6 to 14 was blocked. The allow rules in `.claude/settings.local.json` did their job.
- The `pre-push` and `commit-msg` hooks run. Two "fix clippy" commits (`04dfefe`, `588f81d`) show the check caught errors before master.
- CI now runs the Python capture tests.
- Every hardware test reads the old state first and puts it back.

## 1. The plan still lives only outside the repo (Navigation, still open)

**What happened:** each of the 9 threads started with "read the plan to find stage N". Stage 11 spent 1.5 minutes on that step. The coordinator filled the gap with long briefs that copy the repo state (crates, stages done, profile fields). `docs/PLAN.md` and `CLAUDE.md` do not exist.

**Fix:** commit `docs/PLAN.md` with the stage list and a done mark per stage. Add a 1-line `CLAUDE.md`: "Stage plan and status: `docs/PLAN.md`." Then the briefs can be one line: "build stage N".

## 2. A copied fact in the brief went stale (No-ops / stale cache)

**What happened:** the stage 10 brief said "stage 0 restored Balanced". The laptop was on Performance. The agent found it by reading the controller. Stages 11 to 14 then repeat "the laptop was on Performance with fans on auto" in every brief. The hardware tests already read the state before they change it, so this line does no work, and it can be wrong.

**Fix:** remove the laptop-state line from the briefs. The rule stays short: "a hardware test restores what it read before".

## 3. No single command runs the hardware checks (Automated checks)

**What happened:** the hardware tests are `#[ignore]` tests in 2 files. Their comments give 2 different commands. `crates/razer-device/tests/hardware.rs` says `--test-threads=1`. `crates/blade-core/tests/hardware.rs` says only `cargo test -- --ignored`. Without `--test-threads=1`, two tests can talk to the keyboard controller at the same time.

**Fix:** add `.cargo/config.toml` with an alias:
`[alias]` / `hw = "test --workspace -- --ignored --test-threads=1"`.
Then both comments say `cargo hw`.

## 4. Python leaves `capture/__pycache__/` in git status (Automated checks, mechanical)

**What happened:** the new `pre-push` hook runs the Python tests. Python writes `capture/__pycache__/`. `.gitignore` does not list it. It shows as untracked now.

**Fix:** add `__pycache__/` to `.gitignore`.

## 5. "Delete the branch on GitHub" is a no-op step (No-ops)

**What happened:** from stage 7 on, no branch was pushed to GitHub. Every report spends a line on "there was nothing to delete there".

**Fix:** change the project git rule to: "delete the branch locally; delete it on GitHub only if you pushed it". This rule is in the project settings, so you change it there.

## 6. Leftovers from the first retro (still open)

- The old branch `capture/synapse-usb-reports` is still on the laptop and on GitHub.
- `CODING_STANDARDS.md` was never made. The 2 doc lines in `docs/driver-removal.md` are fixed.

## Small, no fix needed

- Stages 11 and 13 got a late second "next". The agent answered twice. No harm.
- Checks only a person can do: static colour has no read-back (you must look at the keys). A real sleep and wake was not tested for stage 13.
