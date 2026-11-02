# synapse

Stage list and status: `docs/PLAN.md`. Hardware checks on the laptop: `cargo hw`.
Project notes (laptop facts, working style, lessons): `docs/NOTES.md`.

## Target
- Rust, Windows 11, Razer Blade 18 (RZ09-0529).

## How to work
- TDD: red, green, refactor, in small stages, per the plan: https://claude.ai/artifact/JSzNYRpwZJyywztJtBSk2P
- Clean code: DRY, small functions, SOLID, YAGNI, KISS.

## Model profiles
- Design so other Razer laptop models can be added later.
- Protocol code stays model-agnostic.
- Everything model-specific lives in one model profile (`crates/razer-proto/src/profile.rs`), as data, not branches in code: USB PID/interface, key grid rows/cols and key positions, which features exist, fan range, display modes.
- 02C7 / Blade 18 2026 is the only profile for now.
- Adding a model should mean adding a profile, not editing core code.
- Don't build speculative features for other models (YAGNI).
