## AgentEvent variant addition checklist
When adding new variant to `src/event.rs`:
1. `bridge.rs` — `translate()` match + `agent_event_kind` test helper
2. `h7_smoke.rs` — `print_event()` match
3. `integration.rs` — `agent_event_kind()` match
4. `src/extras/acp/mod.rs` — ACP event loop match
5. `src/ui/mod.rs` — main handler + `#[cfg(feature = "loop")]` path
6. `src/provider/mod.rs` — `run_print` (wildcard ok)
7. `src/agent/review.rs` — (wildcard ok)
Compiler catches all non-exhaustive patterns. Run `cargo test --bin dirge` (1261 tests) after.
§
## Steering pipeline + dead code + DB patterns
- Steering: UI→interjection_queue→steering_from_queue→LoopMessage::User→MessageStart→bridge→AgentEvent::UserMessage→write_user_lines
- Dead code: NEVER `#![allow(dead_code)]` module-level. Delete legacy. `#[cfg(test)]` for test-only exports. `#[cfg_attr(not(feature))]` for feature gates. `#![allow(unused_imports)]` only in `agent_loop/mod.rs`
- FTS5 formula migration: DELETE FROM messages_fts + INSERT SELECT (NOT 'rebuild' — external content tables don't support it)
- Schema versioning: `PRAGMA user_version`, sequential migrate() checks
- Env var tests: `static ENV_LOCK: Mutex<()>` + `EnvGuard` RAII with Drop cleanup
§
## Learning loop implementation status
Plan at PLAN_LEARNING.md — 14 gaps, 10 rounds. Round 1 DONE (FTS5 tool_name indexing, per-turn DB writes via `persist_turn_to_db()` at Done/Interjected/ContextOverflow/Error boundaries). Rounds 3-6 launched as parallel subagents. Remaining: R2 (trigram FTS5, schema fields), R7 (lineage dedup, FTS5 sanitize), R8 (curator transitions), R9 (actual compression), R10 (skills in system prompt). Verifies: `cargo test --bin dirge` (1261), `cargo check --bin dirge` = 0 warnings.
