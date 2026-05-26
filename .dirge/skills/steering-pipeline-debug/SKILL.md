---
name: steering-pipeline-debug
description: Debugging the steering pipeline — user interjection messages being swallowed or not displayed.
triggers:
  - "user message swallowed"
  - "interjection not showing"
  - "steering message not displayed"
  - "mid-run message not appearing"
---

## Steering pipeline architecture

When the user types while the agent is running, the message flows through 8 steps:

```
UI input → interjection_queue → steering_from_queue → get_steering_messages
→ LoopMessage::User → LoopEvent::MessageStart → EventBridge.translate()
→ AgentEvent::UserMessage → UI event handler → write_user_lines + session.add_message
```

## Common failure points

### 1. Bridge drops User messages (MOST COMMON)
`src/agent/agent_loop/bridge.rs` — `LoopEvent::MessageStart { message }` handler. If the match arm returns `Vec::new()` for `LoopMessage::User`, the message never reaches the UI. Fix: emit `AgentEvent::UserMessage { content }`.

### 2. UI doesn't handle UserMessage
`src/ui/mod.rs` — the main event handler match. Must have an arm for `AgentEvent::UserMessage { content }` that calls `write_user_lines()` and `session.add_message()`.

### 3. Loop-mode path swallows content
`src/ui/mod.rs` ~line 1576 — the `#[cfg(feature = "loop")]` path. Must display the message content with `»` prefix for each line before showing "(queued...)". The original code only showed "loop active — message queued" without the actual text.

### 4. `wrap_steer_user_message` / `MID_TURN_STEER_WRAPPER`
`src/agent/agent_loop/steering.rs` — the wrapper prefix is prepended so the model treats the message as guidance, not a new task. Verify the prefix is present in the queued content.

## Debugging checklist

1. Set a breakpoint at `bridge.rs` `LoopEvent::MessageStart` — is `LoopMessage::User` reaching it?
2. Check what `translate()` returns — is it `Vec::new()` or `AgentEvent::UserMessage`?
3. Check the UI handler — does the `AgentEvent::UserMessage` arm exist and render?
4. Check `steering_from_queue` — is the interjection_queue being drained?
5. Check `MID_TURN_STEER_WRAPPER` — is the wrapper being applied?
