//! Phase 4.5d — adapt dirge's existing Janet plugin hooks
//! (`on-tool-start` + `on-tool-end` with `harness/block`,
//! `harness/mutate-input`, `harness/replace-result` slots) to the
//! pi-style `BeforeToolCallFn` / `AfterToolCallFn` hooks the new
//! loop consumes.
//!
//! Existing dirge wiring lives in `plugin::hook::HookedToolDyn`
//! — that wrapper sits BETWEEN rig and the inner tool. The new
//! loop dispatches tools through `LoopTool` directly, so the
//! `HookedToolDyn` interception point disappears. This module
//! restores plugin observability + mutation by surfacing the
//! same hook dispatches via the new loop's `before_tool_call` /
//! `after_tool_call` config slots.
//!
//! Hook contract preserved verbatim:
//!   - `on-tool-start` may `harness/block` (deny) or
//!     `harness/mutate-input` (rewrite args before execution)
//!   - `on-tool-end` may `harness/replace-result` (rewrite output
//!     before the model sees it)
//!
//! Janet context shape mirrors `HookedToolDyn::call`:
//!   - before: `@{:tool "name" :args "<json-string>"}`
//!   - after:  `@{:tool "name" :output "<text>"}`
//!
//! **Locking pattern**: each hook invocation acquires the
//! `PluginManager` mutex, runs the Janet dispatch synchronously
//! (no `.await` while held), and releases. The 5s `HOOK_TIMEOUT`
//! inside `dispatch_tool_hook` bounds the hold time. This matches
//! the existing `HookedToolDyn` lock pattern exactly.

use std::sync::{Arc, Mutex};

use crate::plugin::{PluginManager, escape_janet_string};

use super::hooks::{
    AfterToolCallContext, AfterToolCallFn, BeforeToolCallContext, BeforeToolCallFn,
    BeforeToolCallReturn,
};
use super::result::{AfterToolCallResult, BeforeToolCallResult};

/// Build a `BeforeToolCallFn` that dispatches `on-tool-start`
/// through the shared `PluginManager`.
///
/// The returned closure:
///   1. Serializes the validated args to JSON (rig's
///      `harness/mutate-input` contract uses JSON strings).
///   2. Locks the manager, calls `dispatch_tool_hook("on-tool-start",
///      ctx)`, releases.
///   3. If `block` is set → returns `BeforeToolCallResult { block:
///      Some(true), reason: Some(msg) }` with the original args.
///   4. If `mutate_input` is set → parses the JSON string back to
///      `Value`; returns the mutated args + no block.
///   5. Otherwise → returns the args unchanged + no block.
///
/// Errors at any step degrade to "no block, original args"
/// rather than failing the tool call. This matches the existing
/// `HookedToolDyn` behavior of tolerating hook errors and
/// continuing — the alternative (failing the tool call on hook
/// errors) would surface as cryptic failures to the user.
pub fn before_hook_from_plugin_manager(pm: Arc<Mutex<PluginManager>>) -> BeforeToolCallFn {
    Arc::new(move |ctx: BeforeToolCallContext| {
        let pm = pm.clone();
        Box::pin(async move {
            // 1. Args → JSON string for the Janet context.
            let args_json = match serde_json::to_string(&ctx.args) {
                Ok(s) => s,
                Err(_) => {
                    // Shouldn't happen for serde_json::Value;
                    // defensive fallback returns original args
                    // unchanged.
                    return BeforeToolCallReturn {
                        result: None,
                        args: ctx.args,
                    };
                }
            };

            // 2. Build context, lock manager, dispatch.
            let janet_ctx = format!(
                "@{{:tool \"{}\" :args \"{}\"}}",
                escape_janet_string(&ctx.tool_call_name),
                escape_janet_string(&args_json),
            );
            let dispatch_result = {
                let mut mgr = pm.lock().unwrap_or_else(|e| e.into_inner());
                mgr.dispatch_tool_hook("on-tool-start", &janet_ctx)
            };

            let hook_result = match dispatch_result {
                Ok(r) => r,
                Err(_) => {
                    // Same tolerance as HookedToolDyn — hook
                    // errors don't fail the tool call.
                    return BeforeToolCallReturn {
                        result: None,
                        args: ctx.args,
                    };
                }
            };

            // 3. Block takes precedence over mutate-input —
            // matches HookedToolDyn ordering (block check fires
            // before mutation is applied to args).
            if let Some(reason) = hook_result.block {
                return BeforeToolCallReturn {
                    result: Some(BeforeToolCallResult {
                        block: Some(true),
                        reason: Some(reason),
                    }),
                    args: ctx.args,
                };
            }

            // 4. Mutate-input: parse the returned JSON string.
            //    If parsing fails, log via tracing and proceed
            //    with the original args (defensive — same
            //    tolerance as before).
            let final_args = if let Some(mutated_json) = hook_result.mutate_input {
                match serde_json::from_str::<serde_json::Value>(&mutated_json) {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::warn!(
                            target: "dirge::agent_loop::plugin_hooks",
                            tool = %ctx.tool_call_name,
                            error = %e,
                            "harness/mutate-input returned invalid JSON; ignoring",
                        );
                        ctx.args
                    }
                }
            } else {
                ctx.args
            };

            BeforeToolCallReturn {
                result: None,
                args: final_args,
            }
        })
    })
}

/// Build an `AfterToolCallFn` that dispatches `on-tool-end`
/// through the shared `PluginManager`.
///
/// The returned closure:
///   1. Extracts a text representation of the tool result for the
///      Janet context (matches `HookedToolDyn::call`'s shape).
///   2. Locks the manager, dispatches `on-tool-end`, releases.
///   3. If `replace_result` is set → returns
///      `Some(AfterToolCallResult { content: Some([new text block]),
///      ... })`. The dispatcher's merge semantics
///      (`finalize_executed_tool_call` at tools.rs) replace the
///      content in full.
///   4. Otherwise → returns `None` (no override).
///
/// `block` / `mutate_input` slots set inside `on-tool-end` are
/// IGNORED — matches HookedToolDyn semantics (line 133:
/// "semantically meaningless past tool exec").
pub fn after_hook_from_plugin_manager(pm: Arc<Mutex<PluginManager>>) -> AfterToolCallFn {
    Arc::new(move |ctx: AfterToolCallContext| {
        let pm = pm.clone();
        Box::pin(async move {
            // 1. Extract text from result.content (matches the
            //    flatten_content shape used by the bridge).
            let output_text = flatten_text(&ctx.result.content);

            // 2. Build context, lock manager, dispatch.
            let janet_ctx = format!(
                "@{{:tool \"{}\" :output \"{}\"}}",
                escape_janet_string(&ctx.tool_call_name),
                escape_janet_string(&output_text),
            );
            let dispatch_result = {
                let mut mgr = pm.lock().unwrap_or_else(|e| e.into_inner());
                mgr.dispatch_tool_hook("on-tool-end", &janet_ctx)
            };

            let hook_result = match dispatch_result {
                Ok(r) => r,
                Err(_) => return None,
            };

            // 3. replace_result → wrap as a single text content
            //    block. Pi's `AfterToolCallResult.content` is
            //    `Vec<TextContent | ImageContent>`; we substitute
            //    a single text block.
            hook_result
                .replace_result
                .map(|new_output| AfterToolCallResult {
                    content: Some(vec![serde_json::json!({
                        "type": "text",
                        "text": new_output,
                    })]),
                    details: None,
                    is_error: None,
                    terminate: None,
                })
        })
    })
}

/// Extract a single text string from the content blocks for the
/// Janet context. Recognises `{type: "text", text: "..."}` blocks;
/// non-text blocks are JSON-stringified.
fn flatten_text(content: &[serde_json::Value]) -> String {
    let mut out = String::new();
    for block in content {
        if let Some(obj) = block.as_object()
            && obj.get("type").and_then(|t| t.as_str()) == Some("text")
            && let Some(text) = obj.get("text").and_then(|t| t.as_str())
        {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(text);
        } else {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&block.to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::agent_loop::message::{AssistantMessage, ContentBlock, StopReason};
    use crate::agent::agent_loop::result::LoopToolResult;
    use serde_json::json;

    /// Skip when Janet VM init fails (e.g. CI without `janet` deps).
    /// Returns `Some(Arc<Mutex<PluginManager>>)` on success.
    fn try_pm() -> Option<Arc<Mutex<PluginManager>>> {
        match PluginManager::try_new() {
            Ok(mgr) => Some(Arc::new(Mutex::new(mgr))),
            Err(_) => None,
        }
    }

    /// Construct a `BeforeToolCallContext` shorthand.
    fn before_ctx(args: serde_json::Value) -> BeforeToolCallContext {
        BeforeToolCallContext {
            assistant_message: AssistantMessage::new(
                vec![ContentBlock::ToolCall {
                    id: "call-1".to_string(),
                    name: "echo".to_string(),
                    arguments: args.clone(),
                }],
                StopReason::ToolUse,
            ),
            tool_call_id: "call-1".to_string(),
            tool_call_name: "echo".to_string(),
            args,
        }
    }

    fn after_ctx(result: LoopToolResult, is_error: bool) -> AfterToolCallContext {
        AfterToolCallContext {
            assistant_message: AssistantMessage::new(vec![], StopReason::ToolUse),
            tool_call_id: "call-1".to_string(),
            tool_call_name: "echo".to_string(),
            args: json!({}),
            result,
            is_error,
        }
    }

    /// Plugin that calls `harness/block` → before-hook returns
    /// `result: Some(block=true)` with the reason.
    #[tokio::test]
    async fn before_hook_blocks_when_plugin_calls_block() {
        let Some(pm) = try_pm() else {
            eprintln!("[skipped] PluginManager::try_new failed (Janet not available)");
            return;
        };
        {
            let mut mgr = pm.lock().unwrap();
            mgr.eval(r#"(defn deny [_ctx] (harness/block "policy violation"))"#)
                .expect("install deny");
            mgr.register("on-tool-start", "deny");
        }

        let hook = before_hook_from_plugin_manager(pm);
        let result = hook(before_ctx(json!({"v": 1}))).await;
        assert!(
            result.result.is_some(),
            "block hook should return a BeforeToolCallResult"
        );
        let inner = result.result.unwrap();
        assert_eq!(inner.block, Some(true));
        assert_eq!(inner.reason.as_deref(), Some("policy violation"));
        // Args unchanged.
        assert_eq!(result.args, json!({"v": 1}));
    }

    /// Plugin that calls `harness/mutate-input` → returned args
    /// reflect the mutation.
    #[tokio::test]
    async fn before_hook_mutates_args_when_plugin_calls_mutate_input() {
        let Some(pm) = try_pm() else {
            eprintln!("[skipped] PluginManager::try_new failed (Janet not available)");
            return;
        };
        {
            let mut mgr = pm.lock().unwrap();
            mgr.eval(
                r#"(defn rewrite [_ctx] (harness/mutate-input "{\"v\":42,\"extra\":\"added\"}"))"#,
            )
            .expect("install rewrite");
            mgr.register("on-tool-start", "rewrite");
        }

        let hook = before_hook_from_plugin_manager(pm);
        let result = hook(before_ctx(json!({"v": 1}))).await;
        assert!(
            result.result.is_none(),
            "mutate-only hook should not produce a block result"
        );
        assert_eq!(result.args, json!({"v": 42, "extra": "added"}));
    }

    /// Plugin that doesn't call any harness fn → no block, args
    /// unchanged.
    #[tokio::test]
    async fn before_hook_noop_when_plugin_does_nothing() {
        let Some(pm) = try_pm() else {
            eprintln!("[skipped] PluginManager::try_new failed (Janet not available)");
            return;
        };
        {
            let mut mgr = pm.lock().unwrap();
            mgr.eval(r#"(defn observer [_ctx] nil)"#)
                .expect("install observer");
            mgr.register("on-tool-start", "observer");
        }

        let hook = before_hook_from_plugin_manager(pm);
        let result = hook(before_ctx(json!({"v": 1}))).await;
        assert!(result.result.is_none());
        assert_eq!(result.args, json!({"v": 1}));
    }

    /// `harness/mutate-input` with malformed JSON → falls back to
    /// original args (logged via tracing). Matches HookedToolDyn's
    /// tolerance of malformed hook output.
    #[tokio::test]
    async fn before_hook_falls_back_on_malformed_mutate_input() {
        let Some(pm) = try_pm() else {
            eprintln!("[skipped] PluginManager::try_new failed (Janet not available)");
            return;
        };
        {
            let mut mgr = pm.lock().unwrap();
            mgr.eval(r#"(defn bad [_ctx] (harness/mutate-input "not-json{"))"#)
                .expect("install bad");
            mgr.register("on-tool-start", "bad");
        }

        let hook = before_hook_from_plugin_manager(pm);
        let result = hook(before_ctx(json!({"v": 1}))).await;
        // Defensive fallback: original args preserved.
        assert!(result.result.is_none());
        assert_eq!(result.args, json!({"v": 1}));
    }

    /// Plugin that calls `harness/replace-result` from
    /// `on-tool-end` → after-hook returns
    /// `Some(AfterToolCallResult { content: Some([new text]), .. })`.
    #[tokio::test]
    async fn after_hook_replaces_result_when_plugin_calls_replace() {
        let Some(pm) = try_pm() else {
            eprintln!("[skipped] PluginManager::try_new failed (Janet not available)");
            return;
        };
        {
            let mut mgr = pm.lock().unwrap();
            mgr.eval(r#"(defn replace [_ctx] (harness/replace-result "rewritten output"))"#)
                .expect("install replace");
            mgr.register("on-tool-end", "replace");
        }

        let hook = after_hook_from_plugin_manager(pm);
        let result = hook(after_ctx(
            LoopToolResult {
                content: vec![json!({"type": "text", "text": "original"})],
                details: json!({}),
                terminate: None,
            },
            false,
        ))
        .await;
        assert!(result.is_some(), "replace-result should produce override");
        let inner = result.unwrap();
        let content = inner.content.expect("content overridden");
        assert_eq!(content.len(), 1);
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[0]["text"], "rewritten output");
        // Other fields untouched (merge semantics).
        assert!(inner.details.is_none());
        assert!(inner.is_error.is_none());
        assert!(inner.terminate.is_none());
    }

    /// Plugin does nothing in `on-tool-end` → after-hook returns
    /// `None` (no override).
    #[tokio::test]
    async fn after_hook_returns_none_when_plugin_does_nothing() {
        let Some(pm) = try_pm() else {
            eprintln!("[skipped] PluginManager::try_new failed (Janet not available)");
            return;
        };
        {
            let mut mgr = pm.lock().unwrap();
            mgr.eval(r#"(defn observer [_ctx] nil)"#)
                .expect("install observer");
            mgr.register("on-tool-end", "observer");
        }

        let hook = after_hook_from_plugin_manager(pm);
        let result = hook(after_ctx(
            LoopToolResult {
                content: vec![json!({"type": "text", "text": "original"})],
                details: json!({}),
                terminate: None,
            },
            false,
        ))
        .await;
        assert!(result.is_none());
    }

    /// `flatten_text` joins multiple text blocks with newlines.
    #[test]
    fn flatten_text_joins_blocks() {
        let blocks = vec![
            json!({"type": "text", "text": "line 1"}),
            json!({"type": "text", "text": "line 2"}),
        ];
        assert_eq!(flatten_text(&blocks), "line 1\nline 2");
    }

    /// `flatten_text` falls back to JSON stringify for unknown
    /// block types (matches the bridge's flatten_content).
    #[test]
    fn flatten_text_stringifies_unknown_blocks() {
        let blocks = vec![json!({"type": "image", "url": "x.png"})];
        let out = flatten_text(&blocks);
        assert!(out.contains("image"));
    }
}
