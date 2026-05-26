---
name: dead-code-cleanup
description: Systematic dead code removal following dirge conventions.
triggers:
  - "clean up dead code"
  - "remove allow(dead_code)"
  - "dead code warnings"
  - "#![allow(dead_code)]"
---

## Dead code cleanup workflow

### Rules (in priority order)

1. **DELETE legacy code** — don't annotate. If the old way is fully replaced, remove it.
2. **`#[cfg(test)]`** for test-only exports, constants, helper functions
3. **`#[cfg_attr(not(feature = "X"), allow(dead_code))]`** for feature-gated items (`plugin`, `acp`)
4. **`#[allow(dead_code)]` + doc comment** for API surface pending integration (port contract)
5. **NEVER `#![allow(dead_code)]` at module level** — hides real dead code

### Exceptions

`agent_loop/mod.rs` uses `#![allow(unused_imports)]` on its re-export block because many re-exports are consumed by tests or external crates, not the binary.

### Verification

```bash
cargo check --bin dirge  # must produce ZERO warnings
cargo test --bin dirge   # all 1259 tests must pass
```

### What was removed in the canonical cleanup

- `MEMORY_REVIEW_PROMPT`, `SKILL_REVIEW_PROMPT` (only `COMBINED_REVIEW_PROMPT` used)
- `ZAgent` type alias (unused)
- `create_client` OpenRouter builder (replaced by `provider::create_client`)
- `ChatMessage` type alias (unused)
- `CHARS_PER_TOKEN_ESTIMATE` constant (unused)
- `steering_from_queue_with_sanitizer` + test (redundant — base fn already sanitizes)
- `LoopError` enum + `run_agent_loop_continue` + 4 tests (legacy pi continue path)
- Module-level `#![allow(dead_code)]` from: `agent_loop/mod.rs`, `lsp/mod.rs`, `ui/box_render.rs`
