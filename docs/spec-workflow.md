# Spec-driven workflow

dirge can track feature work as a spec-driven workflow: agree on *what* to
build before writing *how*, and keep the agreed requirements as living
documentation. The model is inspired by
[OpenSpec](https://github.com/Fission-AI/OpenSpec), but where OpenSpec stores
everything as a tree of markdown files parsed back with regexes, dirge stores
it as rows in the per-project SQLite database — the same store the memory
system uses.

## The model

- **Living specs** are the current truth: a `capability` has `requirements`,
  each with one or more `scenarios` (WHEN/THEN behavior examples).
- A **change** is a unit of work. It carries a proposal (why + what), an
  optional design note, a set of **requirement deltas** against the living
  specs, and a **task** checklist.
- **Archiving** a change folds its deltas into the living specs in a single
  transaction and closes the change.

```
propose ──→ add deltas ──→ add tasks ──→ implement (track status) ──→ archive
                                                                         │
                                              folds deltas into living specs
```

Deltas are one of four operations:

| op | meaning | fields |
|---|---|---|
| `added` | a new requirement | `text`, `scenarios` |
| `modified` | changed behavior — carries the full new content | `text`, `scenarios` |
| `removed` | a deprecated requirement | `reason`, `migration` |
| `renamed` | a name change only | `rename_to` |

## Why SQLite instead of markdown

- **No silent parse failures.** OpenSpec's own docs warn that a task with the
  wrong checkbox, or a scenario written with three `#` instead of four, "fails
  silently." Rows and constraints can't drift that way.
- **Real task status.** Each task has a `pending | in_progress | done |
  blocked` status column, so progress is a query — not a regex over `- [ ]`
  checkboxes.
- **Queryable specs and a transactional archive.** Folding deltas into the
  living specs is one transaction; reading "what does this capability require"
  is a query.

## The `spec` tool

The agent drives the workflow through one action-dispatched tool. Actions:

| action | arguments | effect |
|---|---|---|
| `propose` | `slug`, `why`, `what` (opt. `title`) | create a change (becomes active) |
| `set_field` | `slug`, `field` (`title`/`why`/`what`/`design`), `value` | edit a change field |
| `add_delta` | `slug`, `op`, `capability`, `requirement` (+ delta fields) | record a requirement delta |
| `add_task` | `slug`, `text` (opt. `group_no`, `seq`) | append a task (auto-sequenced) |
| `set_task` | `task_id`, `status` | update task status |
| `archive` | `slug` | fold deltas into the living specs and close the change |
| `status` | `slug` (or none) | inspect one change, or list all |
| `specs` | `capability` (or none) | read living requirements, or list capabilities |

`scenarios` is an array of `{name, when_then}` objects. Archiving is refused
while a change still has open tasks, and a change can't be archived twice.

The bundled `spec-driven-workflow` skill teaches the agent when and how to use
the tool; copy it into your skills directory (see [skills.md](skills.md)) to
have it auto-surface.

## Staying on-spec

Two integrations keep the workflow connected to the rest of dirge:

- **Context injection.** The active change — its why/what/design, recorded
  deltas, and tasks with their status — is injected into the agent's system
  prompt at session start, so a resumed or fresh session knows what it's
  implementing and where it left off without querying the tool first.
- **Archive forms a memory.** When a change is archived, its rationale (why +
  design decisions) is folded into durable project [memory](features.md), so
  the reasoning outlives the change record.

## The `/spec` command

A read-only view for humans:

```
/spec                     list all changes with task progress
/spec <slug>              show one change: proposal, deltas, tasks
/spec specs               list living-spec capabilities
/spec specs <capability>  show a capability's requirements + scenarios
```

## Storage

All state lives in the per-project session database
(`.dirge/sessions/state.db`, schema v11) in the `spec_changes`,
`spec_capabilities`, `spec_requirements`, `spec_scenarios`, `spec_deltas`,
and `spec_tasks` tables. Nothing is written outside that database; there is no
`openspec/`-style directory tree.
