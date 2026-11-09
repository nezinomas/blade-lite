# synapse

Stage list and status: `docs/PLAN.md` (the only place for status). Full plan: https://claude.ai/artifact/JSzNYRpwZJyywztJtBSk2P
Laptop and protocol facts, lessons: `docs/NOTES.md`.
Building a stage or ticket: follow the `stage` skill (`.claude/skills/stage/SKILL.md`).

## Target
- Rust, Windows 11, Razer Blade 18 2025 (RZ09-05299ER4, USB PID 02C7).

## Hardware safety (wins over every other rule here)
- This app drives an expensive laptop. Add every guard that fits: validate all input, clamp values to safe ranges, refuse unknown states, and fail safe (do nothing rather than guess).
- Every stage and ticket is reviewed by subagents before merge (`stage` skill, step 7). The author never reviews their own stage.

## Review tiers
- Both reviewers (`informed-reviewer` and `fresh-reviewer`, `.claude/agents/`) for anything that can change what is sent to the laptop: which command, its bytes or values, its limits, or when it is sent.
- That always includes `crates/razer-proto` (and its model profile), `crates/razer-device`, `crates/blade-core`, `crates/platform`, `crates/blade-dev` and the `cargo hw` tests. In `crates/ui` and `crates/blade-lite` it includes any change to Rust code, and to `.slint` value ranges, bindings or callbacks.
- One reviewer (`informed-reviewer`) only when every changed line is one of these: docs (`docs/`, `*.md`), `.claude/`, `.githooks/`, CI files, `capture/` scripts, `.slint` layout or text, log message text.
- If in doubt, use both.

## Commands
- One-time setup: `git config core.hooksPath .githooks` (pre-push runs the checks below, commit-msg blocks AI attribution).
- Checks, same as CI: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo t` (`cargo test --workspace`, quiet), `python -m unittest discover -s capture`.
- Window tests (in `cargo t`, `crates/blade-lite/src/window_tests`): the window, flyout and popup drawn off screen over fakes. Scripted clicks check what is sent, saved, shown and logged. Pictures are compared with `crates/blade-lite/tests/screens/`; a change fails and leaves the new picture and a diff in `target/screens/`. If the change is meant: `BLESS_SCREENS=1 cargo t`, look at the saved pictures, commit them. On CI the pictures are drawn, not compared (other fonts).
- Hardware tests on the laptop: `cargo hw`. Quit the app first (the tests take the one-copy lock). The charger test must start on battery.
- PowerShell scripts: `powershell -ExecutionPolicy Bypass -File <script>`.

## Git
- One branch per stage, named for it, e.g. `feature/keyboard-static-color`.
- Small logical commits with short messages. No AI attribution: no Co-Authored-By, no session links.
- No PRs. Merge into master with `git merge --no-ff` (never squash, rebase or fast-forward), then push.
- After the push, delete the branch locally. Delete it on GitHub only if it was pushed there.
- If a git step is blocked (merge, push, branch delete), stop and give the user the exact commands.

## Finishing a stage
- Tick it `[x]` in `docs/PLAN.md` in the same branch, with a one-line result. The pre-push hook refuses a `feature/`, `fix/` or `spike/` merge that does not change `docs/PLAN.md`.
- Check the screen yourself with the window tests (look at each changed picture), and add a click test for each new control. Before merge, list only the checks software can't see (`stage` skill, step 8) and ask the user to do them. Checks not done yet go in "Still to check on the laptop" in `docs/PLAN.md`.
- Every report ends with one plain line: what was blocked (permissions, push, merge, branch delete, cargo), or "nothing was blocked".

## Skills
- `/retro` (`.claude/skills/retro`) needs the `mattpocock-skills` plugin installed.

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
