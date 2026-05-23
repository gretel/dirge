use futures::StreamExt;
use rig::agent::{Agent, MultiTurnStreamItem};
use rig::completion::{CompletionModel, Message};
use rig::streaming::{StreamedAssistantContent, StreamingChat};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::agent::recovery::{self, RecoveryPolicy};
use crate::event::AgentEvent;
use crate::session::{MessageRole, Session};

/// Per-chunk read deadline for streaming provider responses. Applied
/// to every `stream.next().await` in both the interactive and
/// `run_print` paths. The reason a finite timeout exists at all:
/// `reqwest`'s default streaming behaviour doesn't detect silently-
/// dropped TCP connections (no RST, no FIN — the socket reads block
/// forever). A finite timeout converts that into a retryable
/// `Network` error so the retry loop in `spawn_agent` can re-issue.
///
/// Original value (120s) was too aggressive for reasoning-heavy
/// models. Claude 3.7 / GPT-5 extended thinking, large tool outputs
/// being processed, and provider load spikes routinely produce
/// 2-4 minute chunk gaps that are NOT failures — the model is
/// thinking. The default is now 5 minutes; users with even longer
/// reasoning budgets can bump it via `stream_chunk_timeout_secs`
/// in config.json.
pub const DEFAULT_STREAM_CHUNK_TIMEOUT_SECS: u64 = 300;

pub struct AgentRunner {
    pub event_rx: mpsc::Receiver<AgentEvent>,
    /// Handle to the spawned tokio task. The UI calls `abort()` on interrupt
    /// so in-flight LLM calls and tool execution actually stop, rather than
    /// running to completion in the background and emitting permission
    /// prompts after the user thought they cancelled.
    pub task: JoinHandle<()>,
    /// Send a unit signal to ask the runner to stop the stream at the next
    /// safe boundary (after the current tool call's result). The runner
    /// emits `AgentEvent::Interjected` with whatever assistant text had
    /// streamed so far, and the UI is responsible for queueing the next
    /// user turn. Unbounded because the signal payload is just `()`.
    /// F20: bounded so a user who hammers the interject keybind
    /// can't fill an unbounded queue while the runner is in a long
    /// LLM call. Only the FIRST signal needs to be received — all
    /// subsequent ones are noise (the runner drains via
    /// `try_recv()` after the first wakeup). 64 is generous; if
    /// the channel is full, `try_send` silently no-ops (we already
    /// have one queued).
    pub interject_tx: mpsc::Sender<()>,
}

pub fn convert_history(session: &Session) -> Vec<Message> {
    use rig::OneOrMany;
    use rig::completion::message::AssistantContent;
    let (summary, first_kept) = session.compacted_context();
    let mut messages = Vec::new();

    if let Some(summary) = summary {
        messages.push(Message::system(format!(
            "[Previous conversation summary]\n{}",
            summary
        )));
    }

    for msg in &session.messages[first_kept..] {
        match msg.role {
            MessageRole::User => messages.push(Message::user(msg.content.to_string())),
            MessageRole::System => messages.push(Message::system(msg.content.to_string())),
            MessageRole::Assistant => {
                // Phase 3: if this assistant message has structured
                // tool calls, emit a single Assistant message with
                // text + tool_use content parts, followed by ONE
                // tool_result User message per call. The pairing
                // matches opencode's `toModelMessagesEffect`
                // (`message-v2.ts:630-899`); Anthropic + OpenAI
                // reject orphan tool_use blocks so we always emit a
                // result, marking Interrupted/Failed as error text
                // rather than skipping. Bare assistant messages
                // (no tool_calls) keep the prior simple shape.
                if msg.tool_calls.is_empty() {
                    messages.push(Message::assistant(msg.content.to_string()));
                    continue;
                }

                // Build the Assistant message's content blocks: text
                // first (if any) then each ToolCall.
                let mut parts: Vec<AssistantContent> = Vec::new();
                if !msg.content.is_empty() {
                    parts.push(AssistantContent::text(msg.content.to_string()));
                }
                for tc in &msg.tool_calls {
                    parts.push(AssistantContent::tool_call(
                        tc.id.clone(),
                        tc.name.clone(),
                        tc.args.clone(),
                    ));
                }
                // OneOrMany::many requires at least one element; we
                // always have at least one ToolCall here since
                // tool_calls is non-empty.
                let content = if parts.len() == 1 {
                    OneOrMany::one(parts.pop().unwrap())
                } else {
                    OneOrMany::many(parts).expect("non-empty parts vec")
                };
                messages.push(Message::Assistant { id: None, content });

                // One User tool_result per call. State maps to:
                //  Completed  → result text verbatim
                //  Interrupted → "[Tool execution was interrupted]"
                //  Failed     → "[Tool error: <message>]"
                for tc in &msg.tool_calls {
                    let body = match &tc.state {
                        crate::session::ToolCallState::Completed { result } => result.clone(),
                        crate::session::ToolCallState::Interrupted => {
                            "[Tool execution was interrupted]".to_string()
                        }
                        crate::session::ToolCallState::Failed { error } => {
                            format!("[Tool error: {}]", error)
                        }
                    };
                    messages.push(Message::tool_result(tc.id.clone(), body));
                }
            }
        }
    }

    messages
}
/// dirge-rmk: emit one stream-json event line to stdout. NDJSON shape
/// matches Claude Code so tooling written against `claude --print
/// --output-format stream-json` works against dirge unchanged.
fn emit_stream_json_event(value: serde_json::Value) {
    if let Ok(s) = serde_json::to_string(&value) {
        println!("{}", s);
        let _ = std::io::Write::flush(&mut std::io::stdout());
    }
}

pub async fn run_print<M, P>(
    agent: &Agent<M, P>,
    prompt: &str,
    max_turns: usize,
    chunk_timeout: std::time::Duration,
    output_format: crate::cli::OutputFormat,
) -> anyhow::Result<String>
where
    M: CompletionModel + 'static,
    M::StreamingResponse: Send + Sync + Unpin + Clone + 'static,
    P: rig::agent::PromptHook<M> + 'static,
{
    let start_instant = std::time::Instant::now();
    let session_id = uuid_v4_simple();
    let mut num_turns: u32 = 0;
    // For Json / StreamJson modes the assistant text is BUFFERED
    // (never streamed inline to stdout) so the JSON envelope is the
    // only thing the user sees on stdout. Text mode keeps the prior
    // streaming behavior.
    let suppress_inline = !matches!(output_format, crate::cli::OutputFormat::Text);

    // StreamJson init event — fires once at startup so downstream
    // tools can pick up cwd/session/model before any turns stream.
    // Ported from maki print.rs:67-75 (InitEvent shape).
    if matches!(output_format, crate::cli::OutputFormat::StreamJson) {
        let cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        emit_stream_json_event(serde_json::json!({
            "type": "system",
            "subtype": "init",
            "cwd": cwd,
            "session_id": session_id,
            "tools": Vec::<String>::new(),
            "model": "",
        }));
    }
    // Retry loop. Print mode (`dirge --print "..."`) is commonly used
    // in scripts and CI where a single transient 502 or rate-limit
    // would otherwise turn a 5-line shell snippet into a flaky one.
    // Use the same RecoveryPolicy as the interactive path.
    //
    // Caveat: we only retry when NO bytes of the response have been
    // emitted to stdout yet. Once a byte is out, retrying would
    // duplicate visible output — better to surface the error and let
    // the script decide whether to re-run. This matches what
    // opencode does for its non-interactive path.
    let policy = RecoveryPolicy::default();
    let mut attempts: usize = 0;
    loop {
        let mut stream = agent
            .stream_chat(prompt.to_string(), Vec::<Message>::new())
            .multi_turn(max_turns)
            .await;

        let mut full_response = String::new();
        let mut had_output = false;
        let mut stream_error: Option<String> = None;

        loop {
            let item = match tokio::time::timeout(chunk_timeout, stream.next()).await {
                Ok(Some(item)) => item,
                Ok(None) => break,
                Err(_) => {
                    stream_error = Some(format!(
                        "stream chunk timed out after {}s (provider stalled or connection silently dropped) — bump `stream_chunk_timeout_secs` in config.json if your model has long reasoning gaps",
                        chunk_timeout.as_secs(),
                    ));
                    break;
                }
            };
            match item {
                Ok(MultiTurnStreamItem::StreamAssistantItem(StreamedAssistantContent::Text(
                    text,
                ))) => {
                    full_response.push_str(&text.text);
                    if !suppress_inline {
                        print!("{}", text.text);
                        let _ = std::io::Write::flush(&mut std::io::stdout());
                    }
                    had_output = true;
                }
                Ok(MultiTurnStreamItem::StreamAssistantItem(
                    StreamedAssistantContent::Reasoning(r),
                )) => {
                    if !suppress_inline {
                        // Json / StreamJson modes: reasoning is the
                        // model's internal thinking — not part of the
                        // user-visible result. Suppressing keeps the
                        // JSON output clean of chain-of-thought
                        // noise.
                        eprint!("{}", r.display_text());
                        let _ = std::io::Write::flush(&mut std::io::stderr());
                    }
                }
                Ok(MultiTurnStreamItem::FinalResponse(_)) => break,
                Ok(_) => {}
                Err(e) => {
                    stream_error = Some(e.to_string());
                    break;
                }
            }
        }

        if let Some(msg) = stream_error {
            let kind = recovery::classify_error(&msg);
            if !had_output && policy.should_retry(attempts, kind) {
                let delay = policy.backoff_duration_for_msg(attempts, &msg);
                eprintln!(
                    "(retry {}/{} in {:.1}s — {:?})",
                    attempts + 1,
                    policy.max_retries(),
                    delay.as_secs_f64(),
                    kind,
                );
                tokio::time::sleep(delay).await;
                attempts += 1;
                continue;
            }
            // Either we already wrote bytes to stdout (can't safely
            // retry without duplicating) or the retry policy says
            // give up. Newline-terminate any in-flight output before
            // the error so the diagnostic doesn't share a line with
            // half a response.
            if had_output {
                println!();
            }
            eprintln!("Error: {}", msg);
            return Err(anyhow::anyhow!("{}", msg));
        }

        // dirge-rmk: turn complete. Bump turn counter; emit per-format
        // closing payload. Ported from maki print.rs:51-64
        // (`PrintResult`) and the StreamJson assistant event shape.
        num_turns += 1;
        match output_format {
            crate::cli::OutputFormat::Text => {
                println!();
            }
            crate::cli::OutputFormat::Json => {
                // Single Claude-shaped result object. `total_cost_usd`
                // is 0.0 until provider cost plumbing lands.
                let result = serde_json::json!({
                    "type": "result",
                    "subtype": "success",
                    "is_error": false,
                    "duration_ms": start_instant.elapsed().as_millis() as u64,
                    "num_turns": num_turns,
                    "result": full_response.clone(),
                    "session_id": session_id,
                    "total_cost_usd": 0.0,
                });
                if let Ok(s) = serde_json::to_string(&result) {
                    println!("{}", s);
                }
            }
            crate::cli::OutputFormat::StreamJson => {
                // Per-turn assistant event + closing result event.
                emit_stream_json_event(serde_json::json!({
                    "type": "assistant",
                    "message": {
                        "role": "assistant",
                        "content": [{"type": "text", "text": full_response.clone()}],
                    },
                    "session_id": session_id,
                }));
                emit_stream_json_event(serde_json::json!({
                    "type": "result",
                    "subtype": "success",
                    "is_error": false,
                    "duration_ms": start_instant.elapsed().as_millis() as u64,
                    "num_turns": num_turns,
                    "result": full_response.clone(),
                    "session_id": session_id,
                    "total_cost_usd": 0.0,
                }));
            }
        }
        return Ok(full_response);
    }
}

/// Generate a UUIDv4-shaped session id without pulling the `uuid`
/// crate (dirge already has enough deps). Random bytes via system
/// time + thread id seeded into a small xorshift.
fn uuid_v4_simple() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let pid = std::process::id() as u64;
    let mut state = nanos.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(pid);
    let mut bytes = [0u8; 16];
    for chunk in bytes.chunks_mut(8) {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let words = state.to_le_bytes();
        chunk.copy_from_slice(&words[..chunk.len()]);
    }
    // Set version (4) + variant (10) bits per RFC 4122.
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15],
    )
}
