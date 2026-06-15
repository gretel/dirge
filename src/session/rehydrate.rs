//! Reconstruct derived panel state from a resumed session's history.
//!
//! The TODOS and MODIFIED panels read from process-global statics
//! (`agent::tools::todo::TODO_LIST`, `agent::tools::modified::MODIFIED_FILES`)
//! that the agent loop mutates as it runs. Those statics are NOT part of the
//! persisted session schema, so a freshly-started process always begins with
//! them empty. On `dirge --session <id>` the conversation replays but those
//! panels come back blank even though the work that filled them is recorded
//! in the message history.
//!
//! The fix: replay the persisted tool calls and re-derive the same state.
//! `write_todo_list` carries its full list in the args (each call replaces the
//! whole list, so last-write-wins); `write` / `edit` / `edit_minified` /
//! `apply_patch` each name the file they touched. We only count tool calls
//! that actually ran to `Completed` — an interrupted or failed edit never
//! marked the file modified live, so it shouldn't on resume either.

use std::path::PathBuf;

use crate::agent::tools::todo::TodoItem;
use crate::session::{Session, ToolCallState};

/// Panel state recovered from a session's tool-call history. Pure data so it
/// can be unit-tested without touching the process-global statics.
pub struct PanelState {
    /// The final todo list (last `write_todo_list` call wins).
    pub todos: Vec<TodoItem>,
    /// Modified files in recency order (most-recently-touched last),
    /// deduped by raw path string.
    pub modified: Vec<PathBuf>,
}

/// Walk the session's messages in order and reconstruct the todo list and
/// modified-files set from completed tool calls.
pub fn derive_panel_state(session: &Session) -> PanelState {
    let mut todos: Vec<TodoItem> = Vec::new();
    let mut modified: Vec<PathBuf> = Vec::new();

    // Re-insert moves the entry to the end so the freshest touch surfaces
    // last, matching `modified::mark_modified`'s IndexSet semantics.
    let mut touch = |raw: &str| {
        let pb = PathBuf::from(raw);
        if let Some(idx) = modified.iter().position(|e| e == &pb) {
            modified.remove(idx);
        }
        modified.push(pb);
    };

    for msg in &session.messages {
        for tc in &msg.tool_calls {
            if !matches!(tc.state, ToolCallState::Completed { .. }) {
                continue;
            }
            match tc.name.as_str() {
                "write" | "edit" | "edit_minified" => {
                    if let Some(p) = tc.args.get("path").and_then(|v| v.as_str()) {
                        touch(p);
                    }
                }
                "apply_patch" => {
                    if let Some(ops) = tc.args.get("operations").and_then(|v| v.as_array()) {
                        for op in ops {
                            if let Some(p) = op.get("path").and_then(|v| v.as_str()) {
                                touch(p);
                            }
                            // A rename's destination is the file that now
                            // exists; surface it too.
                            if let Some(np) = op.get("new_path").and_then(|v| v.as_str()) {
                                touch(np);
                            }
                        }
                    }
                }
                "write_todo_list" => {
                    if let Some(items) = tc.args.get("todos")
                        && let Ok(parsed) = serde_json::from_value::<Vec<TodoItem>>(items.clone())
                    {
                        // Each call REPLACES the whole list.
                        todos = parsed;
                    }
                }
                _ => {}
            }
        }
    }

    PanelState { todos, modified }
}

/// Push the derived state into the process-global panel statics so a resumed
/// session's TODOS and MODIFIED panels reflect where the previous run left
/// off. Clears both first so the panels show exactly this session's state.
pub fn restore_panels(session: &Session) {
    use crate::sync_util::LockExt;

    let state = derive_panel_state(session);

    *crate::agent::tools::todo::TODO_LIST.lock_ignore_poison() = state.todos;

    // Replay through `mark_modified` so canonicalization, dedup, the 256-entry
    // cap and the panel's version counter all match the live write path.
    crate::agent::tools::modified::clear_modified();
    for p in &state.modified {
        crate::agent::tools::modified::mark_modified(p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{MessageRole, Session, SessionMessage, ToolCallEntry, ToolCallState};
    use compact_str::CompactString;

    fn assistant_with_calls(calls: Vec<ToolCallEntry>) -> SessionMessage {
        SessionMessage {
            role: MessageRole::Assistant,
            content: CompactString::from(""),
            estimated_tokens: 0,
            id: crate::session::new_message_id(),
            timestamp: 0,
            tool_calls: calls,
        }
    }

    fn completed(name: &str, args: serde_json::Value) -> ToolCallEntry {
        ToolCallEntry {
            id: "tc".to_string(),
            name: name.to_string(),
            args,
            state: ToolCallState::Completed {
                result: String::new(),
            },
        }
    }

    fn session_with(messages: Vec<SessionMessage>) -> Session {
        let mut s = Session::new("test", "test-model", 1000);
        s.messages = messages;
        s
    }

    #[test]
    fn last_write_todo_list_wins() {
        let first = completed(
            "write_todo_list",
            serde_json::json!({"todos": [
                {"content": "a", "status": "pending", "priority": "high"}
            ]}),
        );
        let second = completed(
            "write_todo_list",
            serde_json::json!({"todos": [
                {"content": "a", "status": "completed", "priority": "high"},
                {"content": "b", "status": "in_progress", "priority": "low"}
            ]}),
        );
        let s = session_with(vec![assistant_with_calls(vec![first, second])]);
        let state = derive_panel_state(&s);
        assert_eq!(state.todos.len(), 2);
        assert_eq!(state.todos[0].status, "completed");
        assert_eq!(state.todos[1].content, "b");
    }

    #[test]
    fn collects_modified_from_write_edit_patch_in_recency_order() {
        let msgs = vec![
            assistant_with_calls(vec![
                completed("write", serde_json::json!({"path": "/proj/a.rs"})),
                completed("edit", serde_json::json!({"path": "/proj/b.rs"})),
            ]),
            assistant_with_calls(vec![
                completed("edit_minified", serde_json::json!({"path": "/proj/c.rs"})),
                completed(
                    "apply_patch",
                    serde_json::json!({"operations": [
                        {"type": "update", "path": "/proj/d.rs"},
                        {"type": "rename", "path": "/proj/e.rs", "new_path": "/proj/f.rs"}
                    ]}),
                ),
                // Re-touch a → it should move to the end.
                completed("edit", serde_json::json!({"path": "/proj/a.rs"})),
            ]),
        ];
        let state = derive_panel_state(&session_with(msgs));
        let paths: Vec<String> = state
            .modified
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            paths,
            vec![
                "/proj/b.rs",
                "/proj/c.rs",
                "/proj/d.rs",
                "/proj/e.rs",
                "/proj/f.rs",
                "/proj/a.rs", // re-touched → last
            ]
        );
    }

    #[test]
    fn ignores_interrupted_and_failed_calls() {
        let interrupted = ToolCallEntry {
            id: "x".to_string(),
            name: "write".to_string(),
            args: serde_json::json!({"path": "/proj/skipped.rs"}),
            state: ToolCallState::Interrupted,
        };
        let failed = ToolCallEntry {
            id: "y".to_string(),
            name: "write_todo_list".to_string(),
            args: serde_json::json!({"todos": [
                {"content": "nope", "status": "pending", "priority": "high"}
            ]}),
            state: ToolCallState::Failed {
                error: "denied".to_string(),
            },
        };
        let s = session_with(vec![assistant_with_calls(vec![interrupted, failed])]);
        let state = derive_panel_state(&s);
        assert!(state.modified.is_empty());
        assert!(state.todos.is_empty());
    }

    #[test]
    fn empty_session_yields_empty_state() {
        let state = derive_panel_state(&session_with(vec![]));
        assert!(state.todos.is_empty());
        assert!(state.modified.is_empty());
    }

    #[test]
    fn non_file_tools_are_ignored() {
        let s = session_with(vec![assistant_with_calls(vec![
            completed("bash", serde_json::json!({"cmd": "ls"})),
            completed("read", serde_json::json!({"path": "/proj/readonly.rs"})),
        ])]);
        let state = derive_panel_state(&s);
        assert!(state.modified.is_empty());
    }
}
