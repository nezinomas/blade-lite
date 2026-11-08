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
- 02C7 / Blade 18 2025 (RZ09-05299ER4) is the only profile for now. OpenRazer lists 02E1 as the 2026 model.
- Adding a model should mean adding a profile, not editing core code.
- Don't build speculative features for other models (YAGNI).

## Coding principles (Karpathy guidelines)
Source: https://github.com/forrestchang/andrej-karpathy-skills

Behavioral guidelines to reduce common LLM coding mistakes. Merge with project-specific instructions as needed.

**Tradeoff:** These guidelines bias toward caution over speed. For trivial tasks, use judgment.

### 1. Think Before Coding

**Don't assume. Don't hide confusion. Surface tradeoffs.**

Before implementing:
- State your assumptions explicitly. If uncertain, ask.
- If multiple interpretations exist, present them - don't pick silently.
- If a simpler approach exists, say so. Push back when warranted.
- If something is unclear, stop. Name what's confusing. Ask.

### 2. Simplicity First

**Minimum code that solves the problem. Nothing speculative.**

- No features beyond what was asked.
- No abstractions for single-use code.
- No "flexibility" or "configurability" that wasn't requested.
- No error handling for impossible scenarios.
- If you write 200 lines and it could be 50, rewrite it.

Ask yourself: "Would a senior engineer say this is overcomplicated?" If yes, simplify.

### 3. Surgical Changes

**Touch only what you must. Clean up only your own mess.**

When editing existing code:
- Don't "improve" adjacent code, comments, or formatting.
- Don't refactor things that aren't broken.
- Match existing style, even if you'd do it differently.
- If you notice unrelated dead code, mention it - don't delete it.

When your changes create orphans:
- Remove imports/variables/functions that YOUR changes made unused.
- Don't remove pre-existing dead code unless asked.

The test: Every changed line should trace directly to the user's request.

### 4. Goal-Driven Execution

**Define success criteria. Loop until verified.**

Transform tasks into verifiable goals:
- "Add validation" → "Write tests for invalid inputs, then make them pass"
- "Fix the bug" → "Write a test that reproduces it, then make it pass"
- "Refactor X" → "Ensure tests pass before and after"

For multi-step tasks, state a brief plan:
```
1. [Step] → verify: [check]
2. [Step] → verify: [check]
3. [Step] → verify: [check]
```

Strong success criteria let you loop independently. Weak criteria ("make it work") require constant clarification.

**These guidelines are working if:** fewer unnecessary changes in diffs, fewer rewrites due to overcomplication, and clarifying questions come before implementation rather than after mistakes.
