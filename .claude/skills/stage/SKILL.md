---
name: stage
description: Build one stage or ticket from docs/PLAN.md end to end (branch, TDD, two-reviewer check, merge, plan tick, report). Use when the user says "next", "next ticket", names a stage or ticket number, or starts a batch run.
---

# Build a stage

1. Read `CLAUDE.md`, then find the stage in `docs/PLAN.md` and its brief and tests in the full plan artifact. Take the number from `PLAN.md`, not from memory or a brief.
2. Check the plan against the code: is anything it relies on missing or different?
3. Branch from an up-to-date master, named for the stage (`feature/...` or `fix/...`).
4. TDD in small commits: red test, green code, refactor. Short commit messages, no AI attribution.
5. Add every hardware guard that fits (see "Hardware safety" in `CLAUDE.md`).
6. Run the checks from `CLAUDE.md`. Run `cargo hw` when the stage touches the controller (quit the app first).
7. Review with two separate subagents, never yourself. Write the diff with `git diff master...HEAD > target/review.diff`, then run both (in parallel):
   - `informed-reviewer` (`.claude/agents/informed-reviewer.md`): give it the stage brief and the diff path. It reads the plan, notes and repo, checks the diff for bugs and missing guards, and hunts edge cases and paths unsafe for the hardware.
   - `fresh-reviewer` (`.claude/agents/fresh-reviewer.md`): give it only the diff path, no brief. It checks the code against the open-source protocol findings in `docs/protocol-findings.md` (copy of https://claude.ai/artifact/W9Z1Dxcp4Agbxd3r2tEtwX).
   Fix what they find, or add it to `PLAN.md` as a new ticket.
8. Tick the stage `[x]` in `PLAN.md` in the same branch, with a one-line result. Add new laptop facts to `docs/NOTES.md`.
9. `git merge --no-ff` into master, push, delete the branch locally (and on GitHub only if it was pushed).
10. Report: the result, each reviewer's findings listed separately, what still needs the user, and plainly whether anything was blocked (permissions, push, merge, branch delete, cargo) or "nothing was blocked".

In a batch run, repeat for each stage and stop early when a step needs the user. Skip tickets that need the user (see "Working style" in `docs/NOTES.md`).
