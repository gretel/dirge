Legacy: run_agent_loop_continue + LoopError removed. New way: mid-run steering via get_steering_messages. Interjected variant IS actively constructed by bridge (rig stream cancellation). Don't mark dead_code.
§
Learning loop gaps: R1 fixed — per-turn session DB writes + FTS5 tool_name/tool_calls indexing + v2 backfill migration. Remaining: compression (fold flag unused), skill usage tracking, fuzzy patches, curator stub, skills in preamble. 14-gap audit in PLAN_LEARNING.md.
§
## FTS5 formula migration: 'rebuild' doesn't work
External-content FTS5: `INSERT INTO fts(fts) VALUES('rebuild')` re-indexes using old trigger formula. To change indexed content (e.g. add tool_name to index), DELETE FROM fts then INSERT INTO fts SELECT id, new_formula FROM messages.
§
## #![allow(dead_code)] hides real dead code
Module-level suppression in agent_loop/mod.rs and lsp/mod.rs concealed ~50 genuinely unused items. Removing it revealed the true extent. Prefer targeted per-item annotations — even many are better than module-wide silence.
§
## env::set_var + parallel tests = flaky
`std::env::set_var` is global/unsafe/unsynchronized. Tests mutating same key race. Fix: static Mutex + RAII EnvGuard that clears on Drop (applied in dirge_paths.rs).
