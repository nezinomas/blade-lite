# synapse

Stage list and status: `docs/PLAN.md` (the only place for status). Full plan: https://claude.ai/artifact/JSzNYRpwZJyywztJtBSk2P
Laptop and protocol facts, lessons: `docs/NOTES.md`.
Building a stage or ticket: follow the `stage` skill (`.claude/skills/stage/SKILL.md`).

## Target
- Rust, Windows 11, Razer Blade 18 2025 (RZ09-05299ER4, USB PID 02C7).

## Hardware safety (wins over every other rule here)
- This app drives an expensive laptop. Add every guard that fits: validate all input, clamp values to safe ranges, refuse unknown states, and fail safe (do nothing rather than guess).
- Two separate subagents review every stage before merge (see the `stage` skill). The author never reviews their own stage.

## Commands
- One-time setup: `git config core.hooksPath .githooks` (pre-push runs the checks below, commit-msg blocks AI attribution).
- Checks, same as CI: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `python -m unittest discover -s capture`.
- Hardware tests on the laptop: `cargo hw`. Quit the app first (the tests take the one-copy lock). The charger test must start on battery.
- PowerShell scripts: `powershell -ExecutionPolicy Bypass -File <script>`.

## Git
- One branch per stage, named for it, e.g. `feature/keyboard-static-color`.
- Small logical commits with short messages. No AI attribution: no Co-Authored-By, no session links.
- No PRs. Merge into master with `git merge --no-ff` (never squash, rebase or fast-forward), then push.
- After the push, delete the branch locally. Delete it on GitHub only if it was pushed there.

## How to work
- TDD: red, green, refactor, in small stages, per the plan.
- Clean code: DRY, small functions, SOLID, YAGNI, KISS.
- State your assumptions. If something changes the goal or the hardware's safety, stop and ask. Otherwise take the most likely reading, say which, and carry on.
- If a simpler approach exists, say so and push back when warranted.
- If 200 lines could be 50, rewrite it. No abstraction for single-use code, no configurability nobody asked for.
- Touch only what the task needs. Don't reformat, refactor or "improve" adjacent code; match the existing style.
- Remove imports and code your change made unused. Mention unrelated dead code, don't delete it.
- Every changed line should trace back to the task.
(Parts adapted from https://github.com/forrestchang/andrej-karpathy-skills)

## Model profiles
- Design so other Razer laptop models can be added later.
- Protocol code stays model-agnostic.
- Everything model-specific lives in one model profile (`crates/razer-proto/src/profile.rs`), as data, not branches in code: USB PID/interface, key grid rows/cols and key positions, which features exist, fan range, display modes.
- 02C7 / Blade 18 2025 is the only profile for now. OpenRazer lists 02E1 as the 2026 model.
- Adding a model should mean adding a profile, not editing core code.
- Don't build speculative features for other models (YAGNI).
