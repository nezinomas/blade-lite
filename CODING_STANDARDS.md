# Coding standards

The `informed-reviewer` checks each diff against these rules. They are judgement calls. Rules a machine can check live in clippy instead (`Cargo.toml`, `clippy.toml`, the `#![deny]` line in each `lib.rs`). Hardware safety and the model profile rules are in `CLAUDE.md`.

## Errors and the log
- In `blade-lite`, every error goes through `report()`, so it reaches the log file.
- An error on a path that repeats (a poll, a refresh, a watch) is said once, not on each pass (`ShownErrors`, `SaidLately`).
- Error text is one plain line. Details that change, such as a transaction ID or bytes, go in brackets: `SaidLately` groups repeated errors by the text before the first `(` or `{`.
- `expect` only on values that can't come from the device or a file, with a message that says why it can't fail. Device data never panics.

## State
- A change the laptop took is kept in memory, even if the config save fails (ticket 57). The error says it was sent but not saved.
- After each click, a UI switch is set again from the config, even when the change failed, so the switch never shows a state the app does not hold.
- A send that may have reached the controller is never sent again to "retry" it. One exception (ticket 88): after a failed start, preset change or switch, the whole preset (absolute values, checked again) is sent anew on up to 3 later polls, never in the tick of the failure or the next, so the laptop does not keep a preset the app does not show, such as Performance on battery.

## Tests
- A hardware test reads the state first and puts it back at the end, even when a step fails: keep each `Result`, put the state back, then assert.
- A test for a guard shows the guard sends nothing: it checks what the fake transport got, not only the result.

## Project
- A new dependency in the build gets its licence entry in `THIRD_PARTY_LICENSES.md` in the same change.
