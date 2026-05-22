//! Phase 4.5a — adapter from rig's `StreamingCompletionResponse`
//! to our pi-style `Stream<StreamEvent>`.
//!
//! Rig's lower-level streaming API
//! (`CompletionModel::stream(request)`) yields a
//! `Stream<Result<StreamedAssistantContent<R>, CompletionError>>`.
//! Rig DOES NOT dispatch tools at this level — that's the multi-
//! turn agent's job. Single-turn raw streaming is exactly what we
//! need for our own loop to drive turns.
//!
//! This module ports the wire-level event mapping; the
//! **input-side** adapter (build `CompletionRequest` from
//! `LlmContext`) lands in a follow-up sub-phase since it touches
//! tool definitions + message-shape conversion.
//!
//! Event mapping (rig `StreamedAssistantContent<R>` → pi `StreamEvent`):
//!
//! | Rig variant                          | Pi event                          |
//! |--------------------------------------|-----------------------------------|
//! | (synthesized at stream begin)        | `Start { partial: empty msg }`    |
//! | `Text(t)`                            | `Delta { phase: TextStart/Delta }`|
//! | `Reasoning(r)` (complete block)      | `Delta { phase: ThinkingEnd }`    |
//! | `ReasoningDelta { .. }`              | `Delta { phase: ThinkingStart/Delta }`|
//! | `ToolCall { tool_call, .. }`         | `Delta { ToolCallStart + End }`   |
//! | `ToolCallDelta { content, .. }`      | `Delta { phase: ToolCallStart/Delta }`|
//! | `Final(R)`                           | (silent — captured in Done's reason)|
//! | stream end                           | `Done { reason, message }`        |
//! | `Err(CompletionError)`               | `Error { error }`                 |
//!
//! Partial-message accumulation: the adapter builds up an
//! `AssistantMessage` incrementally as deltas arrive, mirroring
//! pi's `partialMessage` in agent-loop.ts:310-340. Each `Delta`
//! event carries the running partial so consumers can render
//! incremental updates.

use std::pin::Pin;

use async_stream::stream;
use futures::Stream;
use futures::stream::StreamExt;
use rig::completion::{CompletionError, GetTokenUsage};
use rig::streaming::{StreamedAssistantContent, StreamingCompletionResponse};

use super::message::{AssistantMessage, ContentBlock, DeltaPhase, StopReason, StreamEvent};

/// Wrap a rig `StreamingCompletionResponse` as a pi-style stream
/// of `StreamEvent`s. Single-turn — rig does NOT dispatch tools
/// from this raw stream; that's our loop's job.
///
/// Algorithm:
///   1. Yield `Start { partial: empty AssistantMessage }`.
///   2. For each rig chunk, accumulate into the partial and yield
///      a `Delta { phase, partial }` event with the running state.
///   3. On stream end (no error), yield `Done { reason, message }`
///      where `message` is the final assembled `AssistantMessage`
///      and `reason` is inferred from the content (`ToolUse` iff
///      any tool call is present, else `Stop`).
///   4. On `Err(CompletionError)`, yield `Error { error }` and
///      stop.
pub fn wrap_rig_stream<R>(
    rig_stream: StreamingCompletionResponse<R>,
) -> Pin<Box<dyn Stream<Item = StreamEvent> + Send>>
where
    R: Clone + Unpin + Send + GetTokenUsage + 'static,
{
    wrap_streamed_assistant(Box::pin(rig_stream))
}

/// Lower-level variant: wrap any `Stream<Result<StreamedAssistantContent<R>,
/// CompletionError>>`. Used by tests to feed canned event
/// sequences; production callers use [`wrap_rig_stream`] directly.
pub fn wrap_streamed_assistant<R>(
    mut raw: Pin<
        Box<dyn Stream<Item = Result<StreamedAssistantContent<R>, CompletionError>> + Send>,
    >,
) -> Pin<Box<dyn Stream<Item = StreamEvent> + Send>>
where
    R: Clone + Unpin + Send + 'static,
{
    Box::pin(stream! {
        // Step 1: synthesize Start with an empty partial. Pi
        // expects the first event to be Start; rig doesn't emit
        // one.
        let mut partial = AssistantMessage::new(Vec::new(), StopReason::Stop);
        yield StreamEvent::Start { partial: partial.clone() };

        let mut current_text_idx: Option<usize> = None;
        let mut current_thinking_idx: Option<usize> = None;
        // Track tool calls under construction so deltas can find
        // their target content block. Keyed by rig's
        // `internal_call_id`.
        let mut tool_indices: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();

        while let Some(item) = raw.next().await {
            match item {
                Ok(StreamedAssistantContent::Text(t)) => {
                    match current_text_idx {
                        Some(idx) => {
                            if let Some(ContentBlock::Text { text: existing }) =
                                partial.content.get_mut(idx)
                            {
                                existing.push_str(&t.text);
                            }
                            yield StreamEvent::Delta {
                                partial: partial.clone(),
                                phase: DeltaPhase::TextDelta,
                            };
                        }
                        None => {
                            current_text_idx = Some(partial.content.len());
                            partial
                                .content
                                .push(ContentBlock::Text { text: t.text.clone() });
                            // Other blocks are interrupted; reset
                            // their indices so subsequent chunks
                            // open fresh blocks.
                            current_thinking_idx = None;
                            yield StreamEvent::Delta {
                                partial: partial.clone(),
                                phase: DeltaPhase::TextStart,
                            };
                        }
                    }
                }
                Ok(StreamedAssistantContent::ReasoningDelta { id: _, reasoning }) => {
                    match current_thinking_idx {
                        Some(idx) => {
                            if let Some(ContentBlock::Thinking { text }) =
                                partial.content.get_mut(idx)
                            {
                                text.push_str(&reasoning);
                            }
                            yield StreamEvent::Delta {
                                partial: partial.clone(),
                                phase: DeltaPhase::ThinkingDelta,
                            };
                        }
                        None => {
                            current_thinking_idx = Some(partial.content.len());
                            partial.content.push(ContentBlock::Thinking { text: reasoning });
                            current_text_idx = None;
                            yield StreamEvent::Delta {
                                partial: partial.clone(),
                                phase: DeltaPhase::ThinkingStart,
                            };
                        }
                    }
                }
                Ok(StreamedAssistantContent::Reasoning(r)) => {
                    // Complete reasoning block emitted in one shot.
                    // `r.content` is `Vec<ReasoningContent>` — a
                    // tagged enum with Text / Encrypted / Redacted /
                    // Summary variants. We surface plain-text and
                    // Summary; encrypted/redacted payloads are
                    // opaque (no display benefit) so we skip them.
                    let text: String = r
                        .content
                        .iter()
                        .filter_map(|c| match c {
                            rig::completion::message::ReasoningContent::Text {
                                text, ..
                            } => Some(text.clone()),
                            rig::completion::message::ReasoningContent::Summary(s) => {
                                Some(s.clone())
                            }
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    partial.content.push(ContentBlock::Thinking { text });
                    current_thinking_idx = None;
                    current_text_idx = None;
                    yield StreamEvent::Delta {
                        partial: partial.clone(),
                        phase: DeltaPhase::ThinkingEnd,
                    };
                }
                Ok(StreamedAssistantContent::ToolCall {
                    tool_call,
                    internal_call_id,
                }) => {
                    // Complete tool call in one shot. Push and
                    // emit start + end for symmetry with pi's
                    // event vocabulary.
                    let idx = partial.content.len();
                    partial.content.push(ContentBlock::ToolCall {
                        id: tool_call.id.clone(),
                        name: tool_call.function.name.clone(),
                        arguments: tool_call.function.arguments.clone(),
                    });
                    tool_indices.insert(internal_call_id, idx);
                    current_text_idx = None;
                    current_thinking_idx = None;
                    yield StreamEvent::Delta {
                        partial: partial.clone(),
                        phase: DeltaPhase::ToolCallStart,
                    };
                    yield StreamEvent::Delta {
                        partial: partial.clone(),
                        phase: DeltaPhase::ToolCallEnd,
                    };
                }
                Ok(StreamedAssistantContent::ToolCallDelta {
                    id,
                    internal_call_id,
                    content,
                }) => {
                    // Streaming tool call. On first delta for this
                    // `internal_call_id` we open the block AND
                    // apply the content together, emitting a
                    // single `ToolCallStart`. Subsequent deltas
                    // merge into the existing block and emit
                    // `ToolCallDelta`. Mirrors the text/thinking
                    // pattern — the "start" event IS the first
                    // chunk, not a separate prologue.
                    let is_first = !tool_indices.contains_key(&internal_call_id);
                    let idx = if is_first {
                        let i = partial.content.len();
                        partial.content.push(ContentBlock::ToolCall {
                            id: id.clone(),
                            name: String::new(),
                            arguments: serde_json::Value::String(String::new()),
                        });
                        tool_indices.insert(internal_call_id.clone(), i);
                        current_text_idx = None;
                        current_thinking_idx = None;
                        i
                    } else {
                        *tool_indices.get(&internal_call_id).unwrap()
                    };
                    if let Some(ContentBlock::ToolCall {
                        id: existing_id,
                        name,
                        arguments,
                    }) = partial.content.get_mut(idx)
                    {
                        apply_tool_call_delta(existing_id, name, arguments, &id, content);
                    }
                    yield StreamEvent::Delta {
                        partial: partial.clone(),
                        phase: if is_first {
                            DeltaPhase::ToolCallStart
                        } else {
                            DeltaPhase::ToolCallDelta
                        },
                    };
                }
                Ok(StreamedAssistantContent::Final(_)) => {
                    // Provider-specific final-response object.
                    // Rig captures it on the
                    // `StreamingCompletionResponse`; we surface
                    // the assembled message in our Done below.
                }
                Err(err) => {
                    yield StreamEvent::Error {
                        error: err.to_string(),
                    };
                    return;
                }
            }
        }

        // Stream ended normally — finalize with the assembled
        // partial. `stop_reason` is `ToolUse` iff any toolCall
        // block is present (pi's stopReason inference for raw
        // provider streams that don't emit a stop reason
        // explicitly), else `Stop`.
        let has_tool_calls = partial
            .content
            .iter()
            .any(|b| matches!(b, ContentBlock::ToolCall { .. }));
        let final_message = AssistantMessage {
            content: partial.content,
            stop_reason: if has_tool_calls {
                StopReason::ToolUse
            } else {
                StopReason::Stop
            },
            error_message: None,
        };
        yield StreamEvent::Done {
            reason: final_message.stop_reason,
            message: final_message,
        };
    })
}

/// Apply a rig `ToolCallDeltaContent` to an in-progress tool
/// call. Rig deltas carry either the tool name (via
/// `ToolCallDeltaContent::Name`) or argument fragments (via
/// `Delta`). Some providers also re-emit the provider-supplied
/// `id` per delta — we update if non-empty.
fn apply_tool_call_delta(
    existing_id: &mut String,
    name: &mut String,
    arguments: &mut serde_json::Value,
    new_id: &str,
    content: rig::streaming::ToolCallDeltaContent,
) {
    use rig::streaming::ToolCallDeltaContent;
    if existing_id.is_empty() && !new_id.is_empty() {
        *existing_id = new_id.to_string();
    }
    match content {
        ToolCallDeltaContent::Name(n) => {
            *name = n;
        }
        ToolCallDeltaContent::Delta(chunk) => {
            // Args are emitted as JSON-string fragments by most
            // providers. We accumulate into a string; downstream
            // code parses lazily when the value is read.
            if let serde_json::Value::String(s) = arguments {
                s.push_str(&chunk);
            } else {
                *arguments = serde_json::Value::String(chunk);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig::completion::message::{Reasoning, ReasoningContent, Text, ToolCall, ToolFunction};
    use rig::streaming::ToolCallDeltaContent;

    /// Minimal R type for tests — needs Clone + Unpin + Send.
    /// We don't actually instantiate it via `Final`.
    #[derive(Clone, Debug)]
    struct TestResponse;

    /// Build a stream from a Vec of canned items.
    fn raw_stream(
        items: Vec<Result<StreamedAssistantContent<TestResponse>, CompletionError>>,
    ) -> Pin<
        Box<
            dyn Stream<Item = Result<StreamedAssistantContent<TestResponse>, CompletionError>>
                + Send,
        >,
    > {
        Box::pin(futures::stream::iter(items))
    }

    /// Drain a wrapped stream into a Vec of events.
    async fn drain(mut s: Pin<Box<dyn Stream<Item = StreamEvent> + Send>>) -> Vec<StreamEvent> {
        let mut out = Vec::new();
        while let Some(e) = s.next().await {
            out.push(e);
        }
        out
    }

    /// Concise per-event label for assertion ergonomics.
    fn label(e: &StreamEvent) -> String {
        match e {
            StreamEvent::Start { .. } => "start".into(),
            StreamEvent::Delta { phase, .. } => format!("delta:{phase:?}"),
            StreamEvent::Done { reason, .. } => format!("done:{reason:?}"),
            StreamEvent::Error { .. } => "error".into(),
        }
    }

    /// Single text response: Start → TextStart → TextDelta → Done.
    #[tokio::test]
    async fn wraps_simple_text_response() {
        let raw = raw_stream(vec![
            Ok(StreamedAssistantContent::Text(Text {
                text: "Hello".to_string(),
            })),
            Ok(StreamedAssistantContent::Text(Text {
                text: " world".to_string(),
            })),
        ]);
        let events = drain(wrap_streamed_assistant(raw)).await;
        let labels: Vec<_> = events.iter().map(label).collect();
        assert_eq!(
            labels,
            vec![
                "start".to_string(),
                "delta:TextStart".to_string(),
                "delta:TextDelta".to_string(),
                "done:Stop".to_string(),
            ]
        );
        match events.last().unwrap() {
            StreamEvent::Done { message, .. } => {
                assert_eq!(message.content.len(), 1);
                match &message.content[0] {
                    ContentBlock::Text { text } => assert_eq!(text, "Hello world"),
                    _ => panic!("expected text"),
                }
            }
            _ => panic!("expected Done last"),
        }
    }

    /// Complete tool call: ToolCallStart + ToolCallEnd, Done with
    /// stopReason=ToolUse.
    #[tokio::test]
    async fn wraps_complete_tool_call() {
        let raw = raw_stream(vec![Ok(StreamedAssistantContent::ToolCall {
            tool_call: ToolCall {
                id: "call_1".to_string(),
                call_id: None,
                function: ToolFunction {
                    name: "echo".to_string(),
                    arguments: serde_json::json!({"value": "hi"}),
                },
                signature: None,
                additional_params: None,
            },
            internal_call_id: "internal_1".to_string(),
        })]);
        let events = drain(wrap_streamed_assistant(raw)).await;
        let labels: Vec<_> = events.iter().map(label).collect();
        assert_eq!(
            labels,
            vec![
                "start",
                "delta:ToolCallStart",
                "delta:ToolCallEnd",
                "done:ToolUse",
            ]
        );
        match events.last().unwrap() {
            StreamEvent::Done { message, .. } => {
                assert_eq!(message.content.len(), 1);
                if let ContentBlock::ToolCall {
                    id,
                    name,
                    arguments,
                } = &message.content[0]
                {
                    assert_eq!(id, "call_1");
                    assert_eq!(name, "echo");
                    assert_eq!(arguments["value"], "hi");
                } else {
                    panic!("expected toolCall");
                }
            }
            _ => panic!("expected Done"),
        }
    }

    /// Streaming tool call: Name delta + arg fragments assembled.
    #[tokio::test]
    async fn wraps_streaming_tool_call_deltas() {
        let raw = raw_stream(vec![
            Ok(StreamedAssistantContent::ToolCallDelta {
                id: "call_2".to_string(),
                internal_call_id: "internal_2".to_string(),
                content: ToolCallDeltaContent::Name("write".to_string()),
            }),
            Ok(StreamedAssistantContent::ToolCallDelta {
                id: "call_2".to_string(),
                internal_call_id: "internal_2".to_string(),
                content: ToolCallDeltaContent::Delta("{\"pa".to_string()),
            }),
            Ok(StreamedAssistantContent::ToolCallDelta {
                id: "call_2".to_string(),
                internal_call_id: "internal_2".to_string(),
                content: ToolCallDeltaContent::Delta("th\":\"/tmp/x\"}".to_string()),
            }),
        ]);
        let events = drain(wrap_streamed_assistant(raw)).await;
        let labels: Vec<_> = events.iter().map(label).collect();
        assert_eq!(
            labels,
            vec![
                "start",
                "delta:ToolCallStart",
                "delta:ToolCallDelta",
                "delta:ToolCallDelta",
                "done:ToolUse",
            ]
        );
        match events.last().unwrap() {
            StreamEvent::Done { message, .. } => {
                if let ContentBlock::ToolCall {
                    id,
                    name,
                    arguments,
                } = &message.content[0]
                {
                    assert_eq!(id, "call_2");
                    assert_eq!(name, "write");
                    assert_eq!(arguments.as_str().unwrap(), "{\"path\":\"/tmp/x\"}");
                } else {
                    panic!("expected toolCall");
                }
            }
            _ => panic!("expected Done"),
        }
    }

    /// Reasoning deltas accumulate into a Thinking block.
    #[tokio::test]
    async fn wraps_reasoning_deltas() {
        let raw = raw_stream(vec![
            Ok(StreamedAssistantContent::ReasoningDelta {
                id: None,
                reasoning: "Let me think".to_string(),
            }),
            Ok(StreamedAssistantContent::ReasoningDelta {
                id: None,
                reasoning: " about this".to_string(),
            }),
        ]);
        let events = drain(wrap_streamed_assistant(raw)).await;
        let labels: Vec<_> = events.iter().map(label).collect();
        assert_eq!(
            labels,
            vec![
                "start",
                "delta:ThinkingStart",
                "delta:ThinkingDelta",
                "done:Stop",
            ]
        );
        match events.last().unwrap() {
            StreamEvent::Done { message, .. } => {
                if let ContentBlock::Thinking { text } = &message.content[0] {
                    assert_eq!(text, "Let me think about this");
                } else {
                    panic!("expected thinking");
                }
            }
            _ => panic!("expected Done"),
        }
    }

    /// Complete reasoning block (one-shot).
    #[tokio::test]
    async fn wraps_complete_reasoning() {
        // `Reasoning` is `#[non_exhaustive]`; use its public
        // constructor.
        let raw = raw_stream(vec![Ok(StreamedAssistantContent::Reasoning(
            Reasoning::new("All thinking"),
        ))]);
        let events = drain(wrap_streamed_assistant(raw)).await;
        assert!(matches!(events[0], StreamEvent::Start { .. }));
        assert!(matches!(
            events[1],
            StreamEvent::Delta {
                phase: DeltaPhase::ThinkingEnd,
                ..
            }
        ));
        assert!(matches!(events[2], StreamEvent::Done { .. }));
    }

    /// Error chunk emits Error and stops the stream.
    #[tokio::test]
    async fn wraps_error_emits_error_and_stops() {
        let raw = raw_stream(vec![
            Ok(StreamedAssistantContent::Text(Text {
                text: "partial".to_string(),
            })),
            Err(CompletionError::ProviderError("bad upstream".to_string())),
            Ok(StreamedAssistantContent::Text(Text {
                text: " more text".to_string(),
            })),
        ]);
        let events = drain(wrap_streamed_assistant(raw)).await;
        assert!(matches!(events.last(), Some(StreamEvent::Error { .. })));
        let dones = events
            .iter()
            .filter(|e| matches!(e, StreamEvent::Done { .. }))
            .count();
        assert_eq!(dones, 0);
    }

    /// Mixed content: text → reasoning → text produces 3 blocks
    /// because the reasoning resets the text-block index.
    #[tokio::test]
    async fn wraps_mixed_content_resets_block_indices() {
        let raw = raw_stream(vec![
            Ok(StreamedAssistantContent::Text(Text {
                text: "hi ".to_string(),
            })),
            Ok(StreamedAssistantContent::ReasoningDelta {
                id: None,
                reasoning: "thinking".to_string(),
            }),
            Ok(StreamedAssistantContent::Text(Text {
                text: "done".to_string(),
            })),
        ]);
        let events = drain(wrap_streamed_assistant(raw)).await;
        let final_msg = events
            .iter()
            .rev()
            .find_map(|e| {
                if let StreamEvent::Done { message, .. } = e {
                    Some(message.clone())
                } else {
                    None
                }
            })
            .expect("Done");
        assert_eq!(final_msg.content.len(), 3);
        assert!(matches!(
            &final_msg.content[0],
            ContentBlock::Text { text } if text == "hi "
        ));
        assert!(matches!(
            &final_msg.content[1],
            ContentBlock::Thinking { text } if text == "thinking"
        ));
        assert!(matches!(
            &final_msg.content[2],
            ContentBlock::Text { text } if text == "done"
        ));
    }
}
