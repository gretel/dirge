---
name: add-agent-event-variant
description: Adding a new variant to the AgentEvent enum in dirge.
triggers:
  - "add a new AgentEvent variant"
  - "new event type for the agent loop"
  - "AgentEvent enum extension"
---

## Adding a new `AgentEvent` variant

When adding a variant to `src/event.rs` `AgentEvent` enum, update ALL exhaustive match arms. The compiler will find most — use `cargo check --bin dirge` to locate them.

### Files to update

1. `src/event.rs` — add the variant
2. `src/agent/agent_loop/bridge.rs` — `translate()` method + `agent_event_kind` test helper (~line 981)
3. `src/agent/agent_loop/h7_smoke.rs` — `print_event()` function
4. `src/agent/agent_loop/integration.rs` — `agent_event_kind()` helper
5. `src/extras/acp/mod.rs` — ACP event loop match
6. `src/ui/mod.rs` — main UI event handler (many arms) + `#[cfg(feature = "loop")]` path
7. `src/provider/mod.rs` — `run_print` path (wildcard `_ => {}`, won't break)
8. `src/agent/review.rs` — (wildcard `_ => {}`, won't break)

### Verification

```bash
cargo test --bin dirge  # 1259 tests must pass
cargo check --bin dirge  # zero warnings
```

### Recent example

Added `AgentEvent::UserMessage { content: CompactString }` to fix steering-injected user messages being swallowed. The bridge at `MessageStart` for `LoopMessage::User` now emits this variant instead of `Vec::new()`.
