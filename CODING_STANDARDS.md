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
- A send that may have reached the controller is never sent again to "retry" it, except in the cases below (user pick, ticket 256, 2026-11-18). Each sends absolute values, checked again, and has a cap on its count or its rate; a new case needs the user's yes and a line here. Not a resend: after an open again's or an updater's pause the preset goes once, with the start (ticket 256).
  - The preset after a failed start, preset change or switch (ticket 88): anew on up to 3 later polls, never in the tick of the failure or the next, so the laptop does not keep a preset the app does not show, such as Performance on battery. While Razer software runs, only a battery preset goes again (ticket 258).
  - While the fan guard is on and the preset is unsure, the 3 tries are filled again at each guard reading (ticket 129): no count cap while the guard is on, one try every other poll.
  - Once the tries ran out, while a manual fan the window does not show may run: the preset each minute (ticket 250), no count cap.
  - A power event sends the preset at once while it is unsure (ticket 87), once per event. Windows tells the source once at register, just after the start, so a failed start's preset can go again about 1 s later.
  - The saved lighting after a charger change: up to 3 tries, at least 10 s apart (tickets 244, 246).
  - The sleep's quit preset at a new sleep notice, once per notice (ticket 191).
  - The fan targets once more 2 s after each manual Custom sent whole (ticket 173; needed after one from auto fans).
  - The keyboard level put back after a screen on: up to 3 tries, at least 10 s apart, counted again after a screen off, a pause, a start or a charger change; the quit tries once more (tickets 241, 253).
  - A refusal by a closed or held controller sent nothing, so it is no send (ticket 132).

## Tests
- A hardware test reads the state first and puts it back at the end, even when a step fails: keep each `Result`, put the state back, then assert.
- A test for a guard shows the guard sends nothing: it checks what the fake transport got, not only the result.

## Project
- A new dependency in the build gets its licence entry in `THIRD_PARTY_LICENSES.md` in the same change.
