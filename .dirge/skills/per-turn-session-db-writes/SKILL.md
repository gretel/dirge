---
name: per-turn-session-db-writes
description: Persisting agent turns to session DB for FTS5 search at every run boundary
triggers:
  - "persist_turn_to_db"
  - "session DB write"
  - "per-turn persistence"
  - "insert_message turn boundary"
---

# Per-Turn Session DB Writes

## Problem

Session DB was only written at `AgentEvent::Done` — the end of a successful run. Interrupted runs (Interjected, ContextOverflow, Error) and mid-session tool calls left no searchable trace.

## Solution

Create a `persist_turn_to_db()` helper that writes the current turn (user prompt + assistant text + tool names + tool results) to the SQLite session DB. Call it at EVERY run boundary.

## Where to call

```rust
// In the UI event loop (src/ui/mod.rs):
match event {
    AgentEvent::Done { response, .. } => {
        // ... existing Done handling ...
        persist_turn_to_db(session, &last_user_prompt, &response, &tool_calls_buf);
    }
    AgentEvent::Interjected { partial_response, .. } => {
        // ... existing Interjected handling ...
        // Call BEFORE tool_calls_buf is consumed by std::mem::take
        persist_turn_to_db(session, &last_user_prompt, &partial_response, &tool_calls_buf);
        session.add_message_with_tool_calls(..., std::mem::take(&mut tool_calls_buf));
    }
    AgentEvent::ContextOverflow { .. } => {
        // ... error display, runner teardown ...
        persist_turn_to_db(session, &last_user_prompt, &response_buf, &tool_calls_buf);
        // Then respawn runner
    }
    AgentEvent::Error(e) => {
        // ... error display ...
        persist_turn_to_db(session, &last_user_prompt, &response_buf, &tool_calls_buf);
    }
}
```

## What to persist

- User message: role="user", content=user_prompt
- Assistant message: role="assistant", content=assistant_text, tool_name=space-joined tool names, tool_calls=JSON-serialized tool calls
- Tool result messages: one per tool call, role="tool", content=result text, tool_name=tool name, tool_call_id=tool call id

## Key invariants

1. Best-effort: DB open/write failures log silently and don't break the session
2. Session insert is idempotent (`INSERT OR IGNORE` on primary key)
3. At Interjected, persist BEFORE `std::mem::take(&mut tool_calls_buf)` — after the take, the buffer is empty
4. Tool result content comes from `ToolCallState::Completed { result }`, `Interrupted`, or `Failed { error }`
