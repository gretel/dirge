# Subagent Dispatch Strategy and Worktree-Isolated Writers

## Status

Implemented coordinator runtime contract. Coordinator dispatch, profile activation, batch delivery, and retry behavior describe the current runtime. Worktree-isolated writer behavior below is the live isolation contract: implementations must preserve these guarantees when isolation is available. This document lists verification to run; it does not claim those commands have passed.

## Scope

The coordinator is available only to the interactive TUI's `BackgroundStore`. It is not enabled for `--print`, ACP, or other noninteractive paths; `full` warns and behaves as `off` there. The manual `/orchestrate` and `/delegate` plugins remain proof-of-concept workflows and must not be combined with core `full` coordination.

The main agent owns planning, durable context, reconciliation, integration, and final verification. It provides the user one final concise summary, not intermediate batch summaries.

## Configuration

```json
{
  "subagent_dispatch_strategy": "full",
  "subagent_write_isolation": "auto"
}
```

Both fields are tolerant `Option<String>` wire values, trimmed and case-normalized:

- `subagent_dispatch_strategy`: `off` (default), `optional`, or `full`. Missing, empty, and invalid values resolve to `off`; invalid values warn and never enable coordination.
- `subagent_write_isolation`: `auto` (default), `worktree`, or `serialize`. Missing, empty, and invalid values resolve to `auto`; invalid values warn.
- Prefer `subagent.tools: "readwrite"`; do not use `"full"`, which is ambiguous with strategy mode.

`optional` permits direct trivial work. When it starts coordinated work, it follows the same batch, retry, isolation, reconciliation, and final-summary rules as `full`. `full` dispatches substantive tier-routed research, implementation, and review work.

Coordinator profiles are resolved from loaded agent definitions, not process-global route state. Readonly and readwrite profiles are deterministic by name; toolless profiles satisfy neither tier. `optional` enables only when both tiers exist, otherwise warns and degrades to `off`. `full` requires both tiers and fails interactive startup with a diagnostic when either is absent; it also fails when tools are disabled. Route resolution occurs at process startup, so `/cd` does not reload routes.

## Dispatch, retry, and batch barrier

While enabled, every `task(background=true)` subagent dispatch is tracked in a coordinator generation, including default and toolless subagents. Background shells use a separate store and are unaffected.

- The first tracked task opens a generation. Later tracked tasks join it until delivery, even if another member has completed.
- A generation is deliverable only after every member is terminal. A missing task record becomes a synthetic failure rather than leaving the barrier open.
- Delivery atomically emits one reconciliation continuation, removes the generation's pending notifications, records history, and closes the generation. The next dispatch opens the next generation.
- Notification draining, pending-notification checks, idle resume, interjections, and normal follow-ups respect this barrier. Partial results do not reach the model or trigger a phantom turn.
- `task_status(wait=true)` does not reveal a partial payload for an active generation; it reports that reconciliation will deliver the result.
- At most four background subagents run in a wave.

`retry_of` is coordinator-only and requires `background=true`. It may name one known failed tracked task after delivery. Completed, running, cancelled, unknown, and already-retried tasks are rejected. Accepting a retry links both records; a second retry is rejected and the main agent repairs the work directly.

In `full`, tier-routed readonly and readwrite work requires `background=true`; inexpensive toolless foreground requests remain allowed. Every readwrite dispatch, foreground or background, passes writer-mutation safety checks.

## Writer isolation

Writer isolation resolves as follows:

- `serialize` uses the parent checkout and permits only one active serialized writer.
- `worktree` requires isolation prerequisites and rejects the writer if any prerequisite is unavailable.
- `auto` uses an isolated worktree when possible; otherwise it logs the reason and serializes writers in the parent checkout.

Isolated writers may run concurrently, subject to the four-subagent cap. Serialized writers cannot overlap, including with a foreground parent-checkout writer.

A worktree requires the `git-worktree` feature, a canonical Git session repository, a clean parent working tree according to `git status --porcelain`, supported sandbox mode, and successful worktree creation. Ignored scratch files do not make that porcelain status dirty. The clean-parent requirement prevents writers from missing uncommitted parent changes because linked worktrees begin at committed `HEAD`.

Names use a safe full task UUID: `dirge-task-<task-uuid>` for both the branch and sibling worktree directory. Creation validates repository, branch, and directory inputs, uses argument vectors and Git `--` separators where supported, and cleans up a newly created worktree if later setup fails.

MicroVM does not support isolated writers: `auto` warns and serializes, `worktree` rejects, and `serialize` remains shared-checkout behavior.

## Rooted writer tools and sandboxing

An isolated writer receives newly constructed, restricted tools rooted at its worktree; it never receives parent shared tool instances.

- File tools resolve relative paths from the root and reject absolute, traversal, ancestor, and symlink escapes outside it.
- Search and navigation tools default omitted paths to the root.
- Bash starts with the worktree as its current directory.
- Tools that cannot enforce the root are excluded.
- The permission checker is cloned and retargeted to the root, so in-root paths are internal; root confinement runs before that checker.

For an isolated writer under Bwrap, the runtime binds `/` read-only, the writer worktree read-write at its original path, and `<main-repo>/.git` read-write at its original path; the parent checkout working tree remains read-only. The command runs from the writer worktree. The `.git` mount is required for linked-worktree metadata, commits, objects, refs, and index updates.

The writer preamble identifies the root and branch, prohibits parent-checkout modification, and requires the writer to inspect status, stage intended changes, make one descriptive commit, and report the commit hash, changed files, tests run, and unresolved issues.

## Preservation and reconciliation

Rust never auto-merges writer branches. The coordinator retains committed worktrees for reconciliation and merges valid branches through normal Git tools after resolving conflicts against the retained plan. Reviewers inspect the integrated parent tree only after that merge.

Clean worktrees without useful committed work may be removed best-effort. Failed, timed-out, and cancelled worktrees are removed only when clean and contain no commits. Dirty worktrees and clean committed worktrees remain for salvage, and `cancel_all()` performs the same retention-aware cleanup. Crash leftovers are recoverable with `git worktree prune`.

The single terminal reconciliation continuation includes every dispatch's original prompt and outcome, retry status, output-relay reference when applicable, and writer branch, path, commits, clean/dirty state, and retained salvage path. Before the next dispatch, repair, merge, or verification decision, the coordinator creates an internal status summary covering completed work, failures, remaining requirements, required verification, and the next action.

## Verification

The runtime has focused checks for configuration tolerance; profile resolution and activation; timeout and route metadata; generation barriers and synthetic failures; retry limits; writer exclusion and isolation fallback; worktree validation and clean-only cleanup; rooted path and Bash confinement; permission scoping; Bwrap commit behavior with an unchanged parent source tree; and reconciliation metadata. Run the relevant checks below when changing those areas.

Run the relevant checks, including:

```bash
cargo fmt --check
cargo test --bin dirge coordinator
cargo test --bin dirge background
cargo test --bin dirge task
cargo test --bin dirge agent_defs
cargo test --bin dirge git_worktree
cargo test --no-default-features --features loop --bin dirge coordinator
cargo clippy --all-targets -- -D warnings
cargo test --bin dirge
```

The no-default-feature coordinator check verifies the unavailable-isolation contract: `auto` serializes and `worktree` reports an actionable error when Git-worktree support is not compiled.
