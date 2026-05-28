# Doltlite FFI Spike — Report

**Question:** can dirge replace SQLite with doltlite via FFI without rewriting
the storage layer, while keeping FTS5 and gaining Git-style history primitives?

**Answer:** yes. Every test in the spike passes on first run.

## Setup

- Platform: macOS arm64 (M-series)
- Doltlite: v0.11.2 release, `doltlite-lib-osx-arm64-0.11.2.zip`
- Method: raw `extern "C"` declarations for ~10 sqlite3_* functions, linked
  against `libdoltlite.dylib` via `build.rs`. No bindgen.
- Effort: ~30 min including the install-name fix.

## What worked

| Test | Result |
|---|---|
| Open DB, create tables (sessions + messages mirroring dirge schema) | ✓ |
| Insert + `SELECT COUNT(*)` | ✓ (3 rows in, 3 rows out) |
| `CREATE VIRTUAL TABLE … USING fts5(...)` | ✓ |
| FTS5 MATCH with `snippet()` highlighting | ✓ (matches `<FTS5>`, `<fts5>` with custom delimiters) |
| FTS5 prefix query (`trig*`) | ✓ (matches `trigram`) |
| `SELECT * FROM dolt_log()` | ✓ (returns the schema-init commit row) |
| `SELECT dolt_add('-A')` + `dolt_commit('-m', '…')` | ✓ (returns the new commit SHA `64b2bff…`) |
| `dolt_diff_messages` (between commits) | ✓ |
| `dolt_history_messages` (per-row history with commit_hash) | ✓ |
| `SELECT dolt_branch('feature-fork')` | ✓ |
| `SELECT * FROM dolt_branches` | ✓ (lists `main` + `feature-fork` at the same SHA) |
| Drop / close DB cleanly | ✓ |

**SQLite API version reported by libdoltlite: `3.54.0`** — the C surface is real
SQLite, served by the prolly backend.

## Integration gotchas (now known)

1. **`install_name` is `/usr/local/lib/libdoltlite.dylib` out of the box.** Apps
   that vendor the lib must `install_name_tool -id "@rpath/libdoltlite.dylib"`
   on it (and ad-hoc resign on macOS), then set the RPATH in their `build.rs`.
   One-time fix per vendored copy.
2. **On-disk format is a directory, not a single file.** A trivial DB with one
   session + four messages produced **29 KB across multiple files** in the
   parent dir. Dirge currently writes a single `state.db`; the migration must
   either pick a "data dir" semantic or keep file artifacts inside a hidden
   subdir.
3. **No published Rust crate.** Bindings are hand-rolled or via `bindgen` against
   `doltlite.h` (which is literally `sqlite3.h`). For dirge's real port we'd want
   `bindgen` to keep parity with rusqlite's API surface.
4. **The lib exports 277 `sqlite3_*` symbols + ~hundreds of `doltlite*`
   internals.** A naive rusqlite-with-system-sqlite link should Just Work if the
   dylib is renamed to `libsqlite3` — that's the lightest possible integration
   if we want to skip writing a new wrapper entirely.

## Path forward (no commitment yet, just an estimate)

- **Stage 1 (~half a day):** Repeat the spike inside dirge using `bindgen` and
  a feature-gated `extras::session_db_dolt` module that mirrors `SessionDb`'s
  public surface. Run the existing `learning_loop_tests` against it.
- **Stage 2 (~1-2 days):** Wire `on_memory_write` → `dolt_commit`, expose
  `dolt_history_*` via a new `memory_history` tool the model can query.
- **Stage 3 (?):** Decide whether sessions get per-fork branches
  (`dolt_branch('session-<sha>')`) or stay on `main` with commits-only.

The blocker is no longer "will it work" — it's "is this worth the operational
ask" (vendoring a 6.5 MB `.dylib`, distributing it on Linux/Windows, GC
hygiene via `dolt_gc()`, etc.). Decision can be made on engineering grounds
rather than FFI uncertainty.

## Recommendation

**Proceed to Stage 1 spike inside dirge** if the history features genuinely
unlock product value (audit trail for plugin providers, time-travel debugging
of memory state). Otherwise, file a "future option" note and stay on
rusqlite — the spike proves the door is open, doesn't oblige us to walk
through it.
