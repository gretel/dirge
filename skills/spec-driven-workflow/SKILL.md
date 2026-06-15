---
name: spec-driven-workflow
description: Use when starting a non-trivial feature or change — align on WHAT before HOW by recording a proposal, requirement deltas, and a task checklist with the `spec` tool, then archive to fold the change into the living specs.
---

# Spec-Driven Workflow

## Overview

For anything bigger than a quick fix, agree on *what* to build before writing
*how*. The `spec` tool tracks this in the project's SQLite store: living specs
(capability → requirement → scenario) are the current truth; a **change**
carries requirement deltas plus a task checklist, and **archiving** folds the
deltas into the living specs.

**Core principle:** specs first, code second. A requirement without a scenario
isn't a spec — it's a wish.

## When to use

- A new feature, a behavior change, or a refactor that changes contracts.
- Skip it for typo fixes, one-line bugfixes, and pure mechanical edits.

## The loop

1. **Research existing specs.** `spec(action: "specs")` to list capabilities,
   `spec(action: "specs", capability: "<name>")` to read current requirements.
   Don't re-propose what already exists.

2. **Propose.** Create the change:
   ```
   spec(action: "propose", slug: "add-dark-mode",
        why: "users want a low-light option",
        what: "theme toggle + persisted preference")
   ```
   `slug` is kebab-case and unique. The change becomes active.

3. **Record requirement deltas** — one per requirement the change touches:
   ```
   spec(action: "add_delta", slug: "add-dark-mode",
        op: "added", capability: "theming", requirement: "User can toggle theme",
        text: "The system SHALL persist the chosen theme across sessions.",
        scenarios: [{name: "toggle", when_then: "WHEN the user flips the toggle THEN the theme switches and is saved"}])
   ```
   - `op`: `added` (new), `modified` (changed behavior — include the FULL new
     text + scenarios), `removed` (include `reason` + `migration`), `renamed`
     (set `rename_to`).
   - Use SHALL/MUST in requirement text. Every added/modified requirement needs
     at least one scenario in WHEN/THEN form.

4. **Break down the work.** Add tasks (auto-sequenced within a group):
   ```
   spec(action: "add_task", slug: "add-dark-mode", text: "Add theme context provider")
   ```

5. **Implement, tracking status.** As you work:
   ```
   spec(action: "set_task", task_id: 3, status: "in_progress")
   spec(action: "set_task", task_id: 3, status: "done")
   ```
   Use `blocked` when you hit a blocker; pause and surface it.

6. **Check state** anytime: `spec(action: "status", slug: "add-dark-mode")` for
   one change, or `spec(action: "status")` to list all.

7. **Archive** once every task is done. This folds the deltas into the living
   specs in one transaction and closes the change:
   ```
   spec(action: "archive", slug: "add-dark-mode")
   ```
   Archiving is refused while tasks are still open.

## Notes

- Keep the proposal's `why` short — motivation, not implementation. Put the
  technical approach in the `design` field via
  `spec(action: "set_field", field: "design", ...)` when the change is
  cross-cutting or has non-obvious trade-offs.
- The living specs are the durable record. Treat each scenario as a testable
  case — it's what a future change (or test) checks against.
