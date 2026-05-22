//! Pi-style agent loop. Faithful port of `pi/packages/agent/src/agent-loop.ts`.
//!
//! Phase 0 lands the value-type surface (enums + shape structs) and
//! the `LoopTool` trait. Nothing in this module is reachable from
//! production code until phase 4 of PLAN.md.
//!
//! Reference paths (read alongside this module — pi is authoritative):
//!   - `~/src/pi/packages/agent/src/types.ts`
//!   - `~/src/pi/packages/agent/src/agent-loop.ts`
//!   - `~/src/pi/packages/agent/test/agent-loop.test.ts`
//!
//! Each file in this directory cites the pi line range it maps to so
//! divergences can be audited against the reference. Pi is the spec —
//! we're not redesigning, we're porting.

// Phase 0 lands the type surface but no production caller yet — phase
// 1+ wires this up. The dead-code lint is correctly noting "deliberate
// API surface with no consumer"; silenced at the module level until
// phase 4 flips the feature default.
#![allow(dead_code)]
// Re-exports for the eventual public API. They look "unused" because
// nothing imports `crate::agent::agent_loop::Foo` yet — phase 1+ will.
#![allow(unused_imports)]

pub mod bridge;
pub mod hooks;
pub mod integration;
pub mod message;
#[cfg(feature = "plugin")]
pub mod plugin_hooks;
pub mod result;
pub mod rig_stream;
pub mod rig_tool;
pub mod run;
pub mod steering;
pub mod stream;
pub mod tool;
pub mod tools;
pub mod types;

pub use bridge::EventBridge;
pub use hooks::{
    AfterToolCallContext, AfterToolCallFn, BeforeToolCallContext, BeforeToolCallFn,
    BeforeToolCallReturn, GetFollowupMessagesFn, GetSteeringMessagesFn, PrepareNextTurnFn,
    ShouldStopAfterTurnFn, TurnHookContext,
};
pub use integration::{LoopRunner, LoopSpawnConfig, spawn_loop_runner};
pub use message::{
    AssistantMessage, ContentBlock, DeltaPhase, LoopEvent, LoopMessage, StopReason, StreamEvent,
    ToolResultMessage, UserMessage,
};
#[cfg(feature = "plugin")]
pub use plugin_hooks::{after_hook_from_plugin_manager, before_hook_from_plugin_manager};
pub use result::{AfterToolCallResult, BeforeToolCallResult, LoopToolResult};
pub use rig_stream::{wrap_rig_stream, wrap_streamed_assistant};
pub use rig_tool::RigToolAdapter;
pub use run::{LoopError, run_agent_loop, run_agent_loop_continue, run_loop};
pub use steering::{steering_from_queue, steering_from_queue_with_sanitizer};
pub use stream::{LlmContext, StreamFn, stream_assistant_response};
pub use tool::LoopTool;
pub use tools::{
    ExecutedToolCallBatch, ToolCall, execute_tool_calls, execute_tool_calls_parallel,
    execute_tool_calls_sequential, extract_tool_calls,
};
pub use types::{
    Context, ConvertToLlmFn, GetApiKeyFn, LoopConfig, QueueMode, ThinkingLevel, ToolExecutionMode,
    TransformContextFn, TurnUpdate,
};
