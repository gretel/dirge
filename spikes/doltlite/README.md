# Doltlite FFI Spike

Throwaway POC proving dirge can link against `libdoltlite.dylib` and reach
both the SQLite-compatible C API (FTS5 included) and the Git/SQL primitives
(`dolt_commit`, `dolt_log`, `dolt_branch`, `dolt_diff_*`, `dolt_history_*`).

See [REPORT.md](REPORT.md) for findings.

## Reproduce

```bash
./vendor/fetch.sh   # downloads libdoltlite for your platform + fixes install_name
cargo run
```

Tested on macOS arm64. The fetch script handles Linux x64 / Linux arm64 too;
Windows would need an analogous handler.

## What's intentionally NOT in this spike

- `bindgen` setup — used hand-rolled `extern "C"` for ~10 sqlite3_* fns.
  A real port would generate bindings from `doltlite.h` (which is the upstream
  `sqlite3.h`).
- `rusqlite`-compatible API surface — would let dirge swap engines without
  rewriting `SessionDb`. Spike skipped it; integration plan covers it.
- Cross-platform build matrix — the spike just confirms the link works on
  the developer's host platform.

## Deletion

This whole directory can be `rm -rf`'d when the integration question is
resolved either way. Nothing in the parent crate depends on it.
