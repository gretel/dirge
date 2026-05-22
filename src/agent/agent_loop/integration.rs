//! Phase 4.5f — compose 4.5a (rig stream) + 4.5b (rig tool) + 4.5c
//! (event bridge) + 4.5d (plugin hooks) + 4.5e (steering) into a
//! single spawn function that returns an `AgentEvent`-emitting
//! runner.
//!
//! `LoopRunner` is the new path's public surface. It's
//! intentionally NOT `AgentRunner` from `runner.rs` because the
//! two paths coexist (per PLAN.md phase 4.5f — gated default
//! comes in 4.5h). The UI side ports happen later; for now this
//! is a parallel runner the rest of the test infrastructure
//! drives.
//!
//! ## Composition diagram
//!
//! ```text
//!                       spawn_loop_runner
//!                              │
//!                              ▼
//!     ┌────────────────────────────────────────────────────┐
//!     │  tokio::spawn:                                     │
//!     │                                                    │
//!     │   build LoopConfig from inputs:                    │
//!     │     • convert_to_llm = passthrough                 │
//!     │     • before_tool_call = plugin_hooks (if pm)      │
//!     │     • after_tool_call = plugin_hooks (if pm)       │
//!     │     • get_steering_messages = steering (if q)      │
//!     │                                                    │
//!     │   build Context { system_prompt, msgs, tools }     │
//!     │                                                    │
//!     │   spawn inner task: run_agent_loop(...)            │
//!     │      └─ emits LoopEvent on internal channel        │
//!     │                                                    │
//!     │   loop:                                            │
//!     │     receive LoopEvent                              │
//!     │     translate via EventBridge → Vec<AgentEvent>    │
//!     │     forward each on caller's event channel         │
//!     │                                                    │
//!     │   when inner task finishes, drain channel + exit   │
//!     └────────────────────────────────────────────────────┘
//! ```
//!
//! ## Phase 4.5f scope
//!
//! - **Does**: compose all sub-phase pieces into one async
//!   pipeline; produce `AgentEvent`s observable by existing UI /
//!   ACP code (via the bridge).
//! - **Does NOT**: wire to a real rig `CompletionModel` (that's
//!   the caller's `stream_fn`; phase 4.5f-2 will add a helper
//!   that builds `stream_fn` from a rig agent + tools). Recovery
//!   / retry on errors (phase 4.5g). Flag-gated dispatch from
//!   `runner.rs` (phase 4.5h).
//!
//! ## AbortSignal
//!
//! The runner exposes its `AbortSignal` so callers can cancel
//! the loop. The existing `AgentRunner.interject_tx` is a
//! different mechanism (graceful stop at tool-result boundary);
//! refining the two into one surface lands in phase 4.5g.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use serde_json::Value;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::event::AgentEvent;

use super::bridge::EventBridge;
use super::message::{LoopMessage, UserMessage};
use super::run::run_agent_loop;
use super::steering::steering_from_queue;
use super::stream::StreamFn;
use super::tool::{AbortSignal, LoopTool};
use super::types::{Context, LoopConfig, QueueMode, ToolExecutionMode};

/// Public handle to a running loop. Mirrors the shape of
/// `runner::AgentRunner` (event channel + task handle + cancel
/// signal) without inheriting from it — both paths coexist.
pub struct LoopRunner {
    /// Channel of `AgentEvent`s. UI / ACP consume from here just
    /// like with the existing `AgentRunner`.
    pub event_rx: mpsc::Receiver<AgentEvent>,
    /// Task driving the loop. Caller can `task.abort()` to force-
    /// kill (alongside or instead of `signal.cancel()`).
    pub task: JoinHandle<()>,
    /// Cooperative cancellation. Tools poll this between steps;
    /// the loop checks it at turn boundaries.
    pub signal: AbortSignal,
}

/// Inputs to `spawn_loop_runner`. Bundled to keep the call sites
/// readable as the number of optional pieces grows.
pub struct LoopSpawnConfig {
    /// Stream function — invoked once per LLM call. Phase 4.5f
    /// tests use mock streams; phase 4.5f-2 builds a real-rig
    /// variant via `wrap_rig_stream`.
    pub stream_fn: StreamFn,

    /// System prompt for every LLM call.
    pub system_prompt: String,

    /// Pre-existing conversation history. The loop appends new
    /// turns; returns the complete `new_messages` Vec when done.
    pub history: Vec<LoopMessage>,

    /// User prompt that starts this run.
    pub initial_prompt: String,

    /// Tool registry. Built via `RigToolAdapter::new(rig_tool)`
    /// for each existing dirge tool, or constructed directly from
    /// a custom `impl LoopTool`.
    pub tools: Vec<Arc<dyn LoopTool>>,

    /// Optional plugin manager. When set, `on-tool-start` and
    /// `on-tool-end` hooks dispatch through `plugin_hooks`.
    #[cfg(feature = "plugin")]
    pub plugin_mgr: Option<Arc<Mutex<crate::plugin::PluginManager>>>,

    /// Optional steering queue. When set, polled at every turn
    /// boundary so user-typed mid-run messages get injected as
    /// new user turns.
    pub steering_queue: Option<Arc<Mutex<VecDeque<String>>>>,

    /// Default tool-execution mode (per-tool overrides win). Pi
    /// defaults to Parallel; existing dirge tools that mutate
    /// shared state (bash, edit, write, apply_patch) should
    /// declare `Sequential` via `RigToolAdapter::with_execution_mode`.
    pub tool_execution: ToolExecutionMode,

    /// Channel capacity for the AgentEvent output. 256 matches
    /// the existing `runner::spawn_agent` choice.
    pub event_channel_capacity: usize,
}

impl LoopSpawnConfig {
    /// Build a minimal config — stream_fn + prompt only; empty
    /// history; no tools; no plugins; no steering; defaults
    /// elsewhere. Useful for tests; production code populates
    /// all fields explicitly.
    pub fn minimal(stream_fn: StreamFn, prompt: impl Into<String>) -> Self {
        Self {
            stream_fn,
            system_prompt: String::new(),
            history: Vec::new(),
            initial_prompt: prompt.into(),
            tools: Vec::new(),
            #[cfg(feature = "plugin")]
            plugin_mgr: None,
            steering_queue: None,
            tool_execution: ToolExecutionMode::Parallel,
            event_channel_capacity: 256,
        }
    }
}

/// Spawn a runner that composes the agent_loop pipeline.
///
/// Returns immediately with a `LoopRunner`; the loop runs on a
/// spawned tokio task and emits `AgentEvent`s on `event_rx`.
pub fn spawn_loop_runner(cfg: LoopSpawnConfig) -> LoopRunner {
    let (event_tx, event_rx) = mpsc::channel::<AgentEvent>(cfg.event_channel_capacity);
    let signal = AbortSignal::new();
    let signal_for_task = signal.clone();

    // Build the LoopConfig at construction so the closure
    // doesn't have to. Plugin / steering hooks are installed if
    // their producers were supplied. `mut` is only required
    // under feature=plugin (the `before_tool_call` /
    // `after_tool_call` slots get assigned in that block);
    // silence the warning otherwise.
    #[cfg_attr(not(feature = "plugin"), allow(unused_mut))]
    let mut loop_config = LoopConfig {
        convert_to_llm: passthrough_converter(),
        transform_context: None,
        get_api_key: None,
        api_key: None,
        tool_execution: cfg.tool_execution,
        before_tool_call: None,
        after_tool_call: None,
        prepare_next_turn: None,
        should_stop_after_turn: None,
        get_steering_messages: cfg
            .steering_queue
            .map(|q| steering_from_queue(q, QueueMode::All)),
        get_followup_messages: None,
    };

    #[cfg(feature = "plugin")]
    {
        if let Some(pm) = cfg.plugin_mgr {
            loop_config.before_tool_call = Some(
                super::plugin_hooks::before_hook_from_plugin_manager(pm.clone()),
            );
            loop_config.after_tool_call =
                Some(super::plugin_hooks::after_hook_from_plugin_manager(pm));
        }
    }

    let context = Context {
        system_prompt: cfg.system_prompt,
        messages: cfg.history.iter().map(loop_message_to_value).collect(),
        tools: cfg.tools,
    };
    let prompts = vec![LoopMessage::User(UserMessage {
        content: cfg.initial_prompt,
    })];
    let stream_fn = cfg.stream_fn;

    let task = tokio::spawn(async move {
        // Inner channel for LoopEvents emitted by run_agent_loop.
        // Capacity matches the outer event channel — assumes each
        // LoopEvent expands to <= a small constant of AgentEvents
        // (typically 1-2 via the bridge).
        let (loop_tx, mut loop_rx) = mpsc::channel(256);
        let event_tx_inner = event_tx.clone();
        let signal_inner = signal_for_task.clone();

        // Spawn the loop itself on a sub-task so we can interleave
        // its emission with our translation pump.
        let loop_handle = tokio::spawn(async move {
            let _final_messages = run_agent_loop(
                prompts,
                context,
                loop_config,
                signal_inner,
                &loop_tx,
                &stream_fn,
            )
            .await;
            // Drop the sender so our pump observes channel close.
            drop(loop_tx);
        });

        // Translation pump: receive LoopEvents, translate, forward.
        let mut bridge = EventBridge::new();
        while let Some(loop_evt) = loop_rx.recv().await {
            for agent_evt in bridge.translate(loop_evt) {
                // If the receiver dropped (UI exited), stop
                // pumping — the loop will still finish naturally
                // since its emit channel uses `let _ = .send`.
                if event_tx_inner.send(agent_evt).await.is_err() {
                    break;
                }
            }
        }
        // Wait for the loop task to finish; ignore JoinError —
        // we already drained its events.
        let _ = loop_handle.await;
        // Explicitly drop the outer sender so the receiver
        // observes channel close even if the consumer is slow.
        drop(event_tx_inner);
    });

    LoopRunner {
        event_rx,
        task,
        signal,
    }
}

/// Convert a `LoopMessage` into the placeholder `Value` shape
/// `Context.messages` carries. Duplicated from `run.rs`'s
/// internal helper because that one is private. Phase 4 plans
/// to swap `Vec<Value>` for a typed message list across the
/// module — when that lands this helper goes away.
fn loop_message_to_value(msg: &LoopMessage) -> Value {
    use super::message::{AssistantMessage, ContentBlock, ToolResultMessage};
    fn assistant_to_value(a: &AssistantMessage) -> Value {
        serde_json::json!({
            "role": "assistant",
            "content": a.content,
            "stopReason": a.stop_reason,
            "errorMessage": a.error_message,
        })
    }
    fn tool_result_to_value(t: &ToolResultMessage) -> Value {
        serde_json::json!({
            "role": "toolResult",
            "toolCallId": t.tool_call_id,
            "toolName": t.tool_name,
            "content": t.content,
            "details": t.details,
            "isError": t.is_error,
        })
    }
    match msg {
        LoopMessage::User(u) => serde_json::json!({
            "role": "user",
            "content": u.content,
        }),
        LoopMessage::Assistant(a) => assistant_to_value(a),
        LoopMessage::ToolResult(t) => tool_result_to_value(t),
        LoopMessage::Custom(v) => v.clone(),
    }
}

/// Pass-through `convert_to_llm`. Phase 4.5f-2 will substitute a
/// rig-aware converter that maps our `LoopMessage` enum to rig's
/// `Message` type for the real-LLM path. For tests with mock
/// streams, the stream_fn doesn't actually consume the messages
/// — passthrough is fine.
fn passthrough_converter() -> super::types::ConvertToLlmFn {
    Arc::new(|messages: &[Value]| messages.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::agent_loop::message::{
        AssistantMessage, ContentBlock, StopReason, StreamEvent,
    };
    use crate::agent::agent_loop::result::LoopToolResult;
    use crate::agent::agent_loop::tool::LoopToolUpdate;
    use std::pin::Pin;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Drain the event channel.
    async fn drain(mut rx: mpsc::Receiver<AgentEvent>) -> Vec<AgentEvent> {
        let mut out = Vec::new();
        while let Some(e) = rx.recv().await {
            out.push(e);
        }
        out
    }

    /// Stream factory returning the supplied messages in order.
    fn canned_factory(responses: Vec<AssistantMessage>) -> StreamFn {
        let counter = Arc::new(AtomicUsize::new(0));
        let responses = Arc::new(responses);
        Arc::new(move |_ctx, _key, _signal| {
            let n = counter.fetch_add(1, Ordering::SeqCst);
            let msg = responses.get(n).cloned().unwrap_or_else(|| {
                AssistantMessage::new(
                    vec![ContentBlock::Text {
                        text: "fallback".to_string(),
                    }],
                    StopReason::Stop,
                )
            });
            let reason = msg.stop_reason;
            Box::pin(futures::stream::iter(vec![StreamEvent::Done {
                reason,
                message: msg,
            }]))
        })
    }

    fn text_response(s: &str) -> AssistantMessage {
        AssistantMessage::new(
            vec![ContentBlock::Text {
                text: s.to_string(),
            }],
            StopReason::Stop,
        )
    }

    fn tool_response(id: &str, name: &str, args: Value) -> AssistantMessage {
        AssistantMessage::new(
            vec![ContentBlock::ToolCall {
                id: id.to_string(),
                name: name.to_string(),
                arguments: args,
            }],
            StopReason::ToolUse,
        )
    }

    /// Mock echo tool used by tool-call tests.
    #[derive(Debug)]
    struct EchoTool;
    impl LoopTool for EchoTool {
        fn name(&self) -> &str {
            "echo"
        }
        fn description(&self) -> &str {
            "Echo"
        }
        fn label(&self) -> &str {
            "Echo"
        }
        fn parameters(&self) -> &Value {
            static EMPTY: std::sync::OnceLock<Value> = std::sync::OnceLock::new();
            EMPTY.get_or_init(|| serde_json::json!({"type": "object"}))
        }
        fn execute<'a>(
            &'a self,
            _id: &'a str,
            args: Value,
            _signal: AbortSignal,
            _on_update: LoopToolUpdate,
        ) -> Pin<Box<dyn Future<Output = Result<LoopToolResult, String>> + Send + 'a>> {
            Box::pin(async move {
                Ok(LoopToolResult {
                    content: vec![serde_json::json!({"type": "text", "text": "ok"})],
                    details: args,
                    terminate: None,
                })
            })
        }
    }

    /// Minimal run: text-only canned response → AgentEvents
    /// include TurnStart / TurnEnd / Done in that order. No
    /// Token events because the canned mock provides the whole
    /// message in one Done event (no incremental TextDelta
    /// stream events); the final text lands on `Done.response`.
    /// A real LLM stream would produce TextDelta events that the
    /// bridge translates to Token chunks — exercised in phase
    /// 4.5a's tests against the rig adapter.
    #[tokio::test]
    async fn spawn_emits_expected_event_sequence_for_text_response() {
        let cfg =
            LoopSpawnConfig::minimal(canned_factory(vec![text_response("Hello world")]), "hi");
        let runner = spawn_loop_runner(cfg);
        let events = drain(runner.event_rx).await;
        let kinds: Vec<&str> = events.iter().map(agent_event_kind).collect();
        for required in ["TurnStart", "TurnEnd", "Done"] {
            assert!(kinds.contains(&required), "missing {required} in {kinds:?}");
        }
        // Final response text lands on Done.
        let done = events
            .iter()
            .find_map(|e| match e {
                AgentEvent::Done { response, .. } => Some(response.clone()),
                _ => None,
            })
            .expect("Done must be emitted");
        assert_eq!(done, "Hello world");
        let _ = runner.task.await;
    }

    /// Multi-turn run with a tool call: assistant emits toolCall
    /// → loop dispatches → second LLM call emits final text.
    /// AgentEvents include ToolCall + ToolStarted + ToolResult.
    #[tokio::test]
    async fn spawn_handles_tool_call_then_final_text() {
        let mut cfg = LoopSpawnConfig::minimal(
            canned_factory(vec![
                tool_response("call-1", "echo", serde_json::json!({"v": 1})),
                text_response("done"),
            ]),
            "go",
        );
        cfg.tools.push(Arc::new(EchoTool));
        cfg.tool_execution = ToolExecutionMode::Sequential;

        let runner = spawn_loop_runner(cfg);
        let events = drain(runner.event_rx).await;
        let kinds: Vec<&str> = events.iter().map(agent_event_kind).collect();
        for required in [
            "TurnStart",
            "ToolCall",
            "ToolStarted",
            "ToolResult",
            "TurnEnd",
            "Done",
        ] {
            assert!(kinds.contains(&required), "missing {required} in {kinds:?}");
        }
        let _ = runner.task.await;
    }

    /// Steering queue produces a mid-run interjection; the
    /// runner's second LLM call sees it. Verifies the full
    /// 4.5e + 4.5f integration.
    #[tokio::test]
    async fn spawn_with_steering_queue_injects_mid_run() {
        let queue = Arc::new(Mutex::new(VecDeque::<String>::new()));
        let queue_writer = queue.clone();

        // Inspector: did the second LLM call see the interrupt?
        let saw = Arc::new(Mutex::new(false));
        let saw_clone = saw.clone();
        let counter = Arc::new(AtomicUsize::new(0));

        let factory: StreamFn = Arc::new(move |llm_ctx, _key, _signal| {
            let n = counter.fetch_add(1, Ordering::SeqCst);
            if n == 1 {
                let found = llm_ctx.messages.iter().any(|m| {
                    m.get("role").and_then(|r| r.as_str()) == Some("user")
                        && m.get("content").and_then(|c| c.as_str()) == Some("interrupt")
                });
                *saw_clone.lock().unwrap() = found;
            } else if n == 0 {
                queue_writer
                    .lock()
                    .unwrap()
                    .push_back("interrupt".to_string());
            }
            let msg = if n == 0 {
                tool_response("call-1", "echo", serde_json::json!({}))
            } else {
                text_response("done")
            };
            let reason = msg.stop_reason;
            Box::pin(futures::stream::iter(vec![StreamEvent::Done {
                reason,
                message: msg,
            }]))
        });

        let mut cfg = LoopSpawnConfig::minimal(factory, "start");
        cfg.tools.push(Arc::new(EchoTool));
        cfg.tool_execution = ToolExecutionMode::Sequential;
        cfg.steering_queue = Some(queue);

        let runner = spawn_loop_runner(cfg);
        let _events = drain(runner.event_rx).await;
        let _ = runner.task.await;

        assert!(
            *saw.lock().unwrap(),
            "steering should have injected the interrupt for the second LLM call"
        );
    }

    /// Aborting via the runner's signal cancels the loop. The
    /// task still completes (because the loop reaches a natural
    /// stopping point) but tools observing the signal can short-
    /// circuit. This test verifies the runner exposes a working
    /// signal — the actual mid-tool cancellation is exercised by
    /// phase 4.5g's recovery wrapper.
    #[tokio::test]
    async fn spawn_exposes_working_abort_signal() {
        let cfg = LoopSpawnConfig::minimal(canned_factory(vec![text_response("hi")]), "x");
        let runner = spawn_loop_runner(cfg);
        // Just verify the signal is observable / clonable.
        let s = runner.signal.clone();
        s.cancel();
        assert!(runner.signal.is_cancelled());
        let _ = runner.task.await;
    }

    /// Plugin-feature: install a `harness/block`-ing plugin;
    /// verify the tool is blocked and the resulting tool result
    /// surfaces as an error.
    #[cfg(feature = "plugin")]
    #[tokio::test]
    async fn spawn_with_plugin_block_hook_blocks_tool() {
        use crate::plugin::PluginManager;
        let pm = match PluginManager::try_new() {
            Ok(mgr) => Arc::new(Mutex::new(mgr)),
            Err(_) => {
                eprintln!("[skipped] PluginManager::try_new failed");
                return;
            }
        };
        {
            let mut mgr = pm.lock().unwrap();
            mgr.eval(r#"(defn deny [_ctx] (harness/block "policy"))"#)
                .unwrap();
            mgr.register("on-tool-start", "deny");
        }

        let factory = canned_factory(vec![
            tool_response("call-1", "echo", serde_json::json!({})),
            text_response("done"),
        ]);
        let mut cfg = LoopSpawnConfig::minimal(factory, "go");
        cfg.tools.push(Arc::new(EchoTool));
        cfg.tool_execution = ToolExecutionMode::Sequential;
        cfg.plugin_mgr = Some(pm);

        let runner = spawn_loop_runner(cfg);
        let events = drain(runner.event_rx).await;
        let _ = runner.task.await;

        // Tool result should be present and convey the block.
        let found_block_text = events.iter().any(|e| match e {
            AgentEvent::ToolResult { output, .. } => output.contains("policy"),
            _ => false,
        });
        assert!(
            found_block_text,
            "expected ToolResult to convey 'policy' block reason; got {events:?}"
        );
    }

    fn agent_event_kind(e: &AgentEvent) -> &'static str {
        match e {
            AgentEvent::Token(_) => "Token",
            AgentEvent::Reasoning(_) => "Reasoning",
            AgentEvent::ToolCall { .. } => "ToolCall",
            AgentEvent::ToolStarted { .. } => "ToolStarted",
            AgentEvent::ToolResult { .. } => "ToolResult",
            AgentEvent::Error(_) => "Error",
            AgentEvent::ContextOverflow { .. } => "ContextOverflow",
            AgentEvent::Done { .. } => "Done",
            AgentEvent::TurnStart { .. } => "TurnStart",
            AgentEvent::TurnEnd { .. } => "TurnEnd",
            AgentEvent::Interjected { .. } => "Interjected",
        }
    }
}
