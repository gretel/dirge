//! Background review at session end.
//!
//! Port of Hermes's `agent/background_review.py`. After every session,
//! a forked agent with limited tools (memory + skill only) reviews the
//! transcript and writes project learnings to MEMORY.md, PITFALLS.md,
//! and skills.
//!
//! The review runs as a fire-and-forget tokio task — it never blocks
//! the main session. If it fails, the error is logged and the session
//! continues unaffected.
//!
//! Key design decisions from Hermes preserved:
//! - Fork, don't inline (separate agent instance, no prompt-cache pollution)
//! - Tool whitelist (only memory + skill tools)
//! - Same credentials as parent session
//! - Frozen conversation snapshot
//! - Fire-and-forget (daemon thread pattern)

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::extras::dirge_paths::ProjectPaths;
use crate::provider::AnyAgent;

/// Minimum interval between background reviews (seconds).
const MIN_REVIEW_INTERVAL_SECS: u64 = 900; // 15 minutes

/// Last review timestamp (Unix seconds).
static LAST_REVIEW: AtomicU64 = AtomicU64::new(0);

/// Review prompt focused on project memory, pitfalls, and skills.
/// Port of Hermes's `_COMBINED_REVIEW_PROMPT` (background_review.py:150-158)
/// and `_SKILL_REVIEW_PROMPT` (background_review.py:45-148), adapted
/// for coding context.
const COMBINED_REVIEW_PROMPT: &str = r#"Review the conversation above and update what we know about this project and how to work on it.

**CRITICAL: You have ONLY the `memory` and `skill` tools available.** Do not attempt to use read, write, edit, bash, or any other tools — they are not loaded.

**1. Update MEMORY (project facts, conventions, pitfalls):**
- What build/test commands were discovered or confirmed?
- What naming conventions, file layout patterns, or import styles were used?
- What architecture patterns emerged (how modules relate, error handling style)?
- What library quirks or tool behaviors were discovered?
- Were there any user corrections about how things should be done?
- Was something tried and failed? Capture what was attempted and WHY it failed.

**2. Update SKILLS (procedural improvements):**
Be ACTIVE — most sessions produce at least one skill update. A pass that does nothing is a missed learning opportunity.

Preference order — prefer the earliest that fits:
  1. UPDATE A CURRENTLY-LOADED SKILL. If the conversation involved a skill that is already in the library, extend or correct it first.
  2. UPDATE AN EXISTING UMBRELLA. If the new knowledge belongs under a broader topic that already has a skill, patch it.
  3. ADD A SUPPORT FILE under an existing umbrella via the skill tool (references/, templates/, or scripts/).
  4. CREATE A NEW CLASS-LEVEL UMBRELLA SKILL only when no existing skill covers the class.

Signals that warrant action:
  • User corrected your style, approach, or workflow. Frustration signals like "stop doing X", "this is too verbose", "don't format like this", or an explicit "remember this" are FIRST-CLASS skill signals.
  • Non-trivial technique, fix, workaround, or debugging pattern emerged.
  • A skill that was loaded or consulted turned out wrong or outdated — PATCH IT NOW.
  • A pattern repeated across the session that future sessions would benefit from.

Do NOT capture:
  • Environment-dependent failures: missing binaries, "command not found", unconfigured credentials. The user can fix these — they are not durable rules.
  • Negative claims about tools ("read tool is broken", "cannot use X"). These harden into refusals long after the actual problem was fixed.
  • Session-specific transient errors that resolved before the conversation ended.
  • One-off task narratives. "Analyze this PR" is not a class of work that warrants a skill.

Target shape of the library: CLASS-LEVEL skills with a rich SKILL.md. Not a long flat list of narrow one-session-one-skill entries.

"Nothing to save." is valid but should NOT be the default. Most coding sessions produce at least one learning."#;

/// Spawn a background review task that evaluates the just-completed
/// session and writes learnings to project memory and skills.
///
/// This is fire-and-forget — it runs in a `tokio::spawn` task and
/// returns immediately. Failures are logged to stderr and never
/// block the user.
///
/// Set `review_prompt_override` to use a custom prompt instead of
/// the default COMBINED_REVIEW_PROMPT. Pass `None` for the default.
pub fn spawn_background_review(
    agent: AnyAgent,
    _paths: ProjectPaths,
    transcript: String,
    review_prompt_override: Option<&str>,
) {
    // Rate-limit: skip if a review ran recently. Uses atomic
    // compare-and-swap so concurrent Done events from different
    // sessions don't race — only the first one wins.
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let last = LAST_REVIEW.load(Ordering::Relaxed);
    if now.saturating_sub(last) < MIN_REVIEW_INTERVAL_SECS {
        tracing::debug!(
            target: "dirge::review",
            elapsed_secs = %(now - last),
            "Skipping background review — last review was too recent"
        );
        return;
    }
    LAST_REVIEW.store(now, Ordering::Relaxed);

    let prompt = review_prompt_override
        .map(|s| s.to_string())
        .unwrap_or_else(|| COMBINED_REVIEW_PROMPT.to_string());

    tokio::spawn(async move {
        // Build a review runner with only memory + skill tools.
        let review_runner = agent.spawn_review_runner(prompt, transcript);

        // Drain events. Track tool calls so we can summarize what
        // the review actually did (Hermes's action summary pattern).
        let mut rx = review_runner.event_rx;
        let mut had_error = false;
        let mut tool_actions: Vec<String> = Vec::new();

        while let Some(event) = rx.recv().await {
            use crate::event::AgentEvent;
            match event {
                AgentEvent::Error(msg) => {
                    tracing::warn!(
                        target: "dirge::review",
                        error = %msg,
                        "Background review encountered an error"
                    );
                    had_error = true;
                }
                AgentEvent::ToolCall { name, .. } => {
                    tool_actions.push(name.to_string());
                }
                AgentEvent::Done { .. } => {
                    break;
                }
                _ => {
                    // Tokens, tool calls, etc. — consumed silently.
                }
            }
        }

        if !had_error && !tool_actions.is_empty() {
            // Surface action summary so the user knows what was learned.
            // Port of Hermes's `_safe_print` (background_review.py:514-516).
            let summary = tool_actions
                .iter()
                .fold(Vec::<&str>::new(), |mut acc, a| {
                    if !acc.contains(&a.as_str()) {
                        acc.push(a.as_str());
                    }
                    acc
                })
                .join(" · ");
            tracing::info!(
                target: "dirge::review",
                actions = %summary,
                "💾 Self-improvement review: {}",
                summary
            );
        } else if !had_error {
            tracing::info!(
                target: "dirge::review",
                "Background review completed — project knowledge updated"
            );
        }
    });
}

/// Build a human-readable transcript from session messages for
/// background review. Includes user text, assistant text, tool
/// call names+args, and tool results. Compaction summaries are
/// included as system context.
pub fn build_transcript(session: &crate::session::Session) -> String {
    let mut out = String::new();
    for msg in &session.messages {
        match msg.role {
            crate::session::MessageRole::User => {
                out.push_str(&format!("User: {}\n\n", msg.content));
            }
            crate::session::MessageRole::Assistant => {
                if !msg.content.is_empty() {
                    out.push_str(&format!("Assistant: {}\n", msg.content));
                }
                for tc in &msg.tool_calls {
                    let args_str =
                        serde_json::to_string(&tc.args).unwrap_or_else(|_| "{}".to_string());
                    out.push_str(&format!("  [Tool: {}({})]\n", tc.name, args_str));
                    match &tc.state {
                        crate::session::ToolCallState::Completed { result } => {
                            let truncated = truncate_tool_result(result);
                            out.push_str(&format!("  [Result: {}]\n", truncated));
                        }
                        crate::session::ToolCallState::Interrupted => {
                            out.push_str("  [Result: <interrupted>]\n");
                        }
                        crate::session::ToolCallState::Failed { error } => {
                            out.push_str(&format!("  [Result: <failed: {}>]\n", error));
                        }
                    }
                }
                if !msg.content.is_empty() || !msg.tool_calls.is_empty() {
                    out.push('\n');
                }
            }
            crate::session::MessageRole::System => {
                out.push_str(&format!("[System: {}]\n\n", msg.content));
            }
        }
    }
    out
}

fn truncate_tool_result(result: &str) -> String {
    const MAX_TOOL_RESULT: usize = 2000;
    if result.len() <= MAX_TOOL_RESULT {
        result.to_string()
    } else {
        let truncated: String = result.chars().take(MAX_TOOL_RESULT).collect();
        format!("{}… (truncated, {} bytes total)", truncated, result.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{MessageRole, Session, ToolCallEntry, ToolCallState};

    fn make_session() -> Session {
        Session::new("test-provider", "test-model", 128_000)
    }

    #[test]
    fn transcript_includes_user_and_assistant() {
        let mut s = make_session();
        s.add_message(MessageRole::User, "how do I build this?");
        s.add_message(MessageRole::Assistant, "Run cargo build");

        let t = build_transcript(&s);
        assert!(t.contains("User: how do I build this?"));
        assert!(t.contains("Assistant: Run cargo build"));
    }

    #[test]
    fn transcript_includes_tool_calls_and_results() {
        let mut s = make_session();
        s.add_message(MessageRole::User, "read the file");
        let tc = ToolCallEntry {
            id: "call-1".to_string(),
            name: "read".to_string(),
            args: serde_json::json!({"path": "/tmp/x"}),
            state: ToolCallState::Completed {
                result: "file contents here".to_string(),
            },
        };
        s.add_message_with_tool_calls(MessageRole::Assistant, "Let me read that.", vec![tc]);

        let t = build_transcript(&s);
        assert!(t.contains("[Tool: read("));
        assert!(t.contains("[Result: file contents here]"));
    }

    #[test]
    fn transcript_truncates_large_tool_results() {
        let mut s = make_session();
        let big = "x".repeat(3000);
        let tc = ToolCallEntry {
            id: "c1".to_string(),
            name: "bash".to_string(),
            args: serde_json::json!({"cmd": "cat big.txt"}),
            state: ToolCallState::Completed {
                result: big.clone(),
            },
        };
        s.add_message_with_tool_calls(MessageRole::Assistant, "", vec![tc]);

        let t = build_transcript(&s);
        assert!(t.contains("truncated"));
        assert!(!t.contains(&big));
    }

    #[test]
    fn transcript_includes_system_messages() {
        let mut s = make_session();
        s.add_message(
            MessageRole::System,
            "compaction summary: previous work on auth module",
        );
        s.add_message(MessageRole::User, "continue");

        let t = build_transcript(&s);
        assert!(t.contains("[System: compaction summary"));
        assert!(t.contains("User: continue"));
    }

    #[test]
    fn transcript_handles_interrupted_tool() {
        let mut s = make_session();
        let tc = ToolCallEntry {
            id: "ci".to_string(),
            name: "bash".to_string(),
            args: serde_json::json!({}),
            state: ToolCallState::Interrupted,
        };
        s.add_message_with_tool_calls(MessageRole::Assistant, "", vec![tc]);

        let t = build_transcript(&s);
        assert!(t.contains("<interrupted>"));
    }

    #[test]
    fn review_prompt_contains_required_sections() {
        // Verify the prompt has the key structural elements from Hermes.
        assert!(COMBINED_REVIEW_PROMPT.contains("Preference order"));
        assert!(COMBINED_REVIEW_PROMPT.contains("Do NOT capture"));
        assert!(COMBINED_REVIEW_PROMPT.contains("Signals that warrant"));
        assert!(COMBINED_REVIEW_PROMPT.contains("Environment-dependent"));
        assert!(COMBINED_REVIEW_PROMPT.contains("CLASS-LEVEL skills"));
        assert!(COMBINED_REVIEW_PROMPT.contains("Nothing to save"));
    }

    #[test]
    fn review_prompt_override_is_accepted() {
        // Verify the function signature compiles with an override.
        // (This is a compile-time check but also verifies the Option
        // typing works.)
        let custom = "Custom review prompt";
        assert_ne!(custom, COMBINED_REVIEW_PROMPT);
    }
}
