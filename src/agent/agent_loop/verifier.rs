//! Pre-finalization verifier gate (F6).
//!
//! Backs the "verify before done" discipline with a *mechanism*, not just
//! prose. It watches tool calls for two facts about the current run: did
//! the agent edit a CODE file, and did it run any shell command (its only
//! route to tests / build / run). When the loop is about to finalize
//! having edited code without ever touching the shell, the gate injects
//! one soft nudge to verify before claiming done — then stays silent
//! (bounded to once per run, so it can never loop).
//!
//! Cheap and signal-based: no extra LLM call, no parsing of command
//! semantics. Self-contained — no rig/LLM state. Lives behind
//! `LoopConfig.verifier`; when `None` the loop behaves byte-identically.
//! Mirrors `FileTouchTracker`: an `Arc<Mutex<Inner>>` fed from the
//! tool-dispatch site and polled at the finalization seam.

use std::sync::{Arc, Mutex};

use super::message::{LoopMessage, UserMessage};

/// One-time nudge injected when the agent is about to finish having
/// edited code without running anything to check it. Soft and bounded —
/// it offers an out so it doesn't fight legitimate "nothing to run" cases.
const VERIFY_NUDGE: &str = "[verify-before-done] You changed code this run but didn't run anything to check it. Before reporting done, run the tests or build to confirm it works. If there's nothing to run, or you verified another way, say so briefly and finish — don't re-edit just to look busy.";

/// Per-run verifier gate. See module docs.
#[derive(Debug)]
pub struct VerifierGate {
    inner: Mutex<Inner>,
}

#[derive(Debug, Default)]
struct Inner {
    /// A mutating file tool touched a code-extension path this run.
    edited_code: bool,
    /// A `bash` tool call ran this run (the only route to tests/build).
    ran_command: bool,
    /// The nudge has already fired — never fire again (bounds the loop).
    fired: bool,
}

impl VerifierGate {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(Inner::default()),
        })
    }

    /// Record a prepared tool call (mirrors `FileTouchTracker`'s hook
    /// site). Flags a code edit when a mutating file tool touches a
    /// code-extension path, and a shell run on any `bash` call.
    pub fn record_tool_call(&self, tool_name: &str, args: &serde_json::Value) {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        match tool_name {
            "write" | "edit" | "apply_patch" => {
                if touches_code_file(args) {
                    inner.edited_code = true;
                }
            }
            "bash" => inner.ran_command = true,
            _ => {}
        }
    }

    /// Finalization seam: returns a one-time verify nudge when code was
    /// edited but no shell command ran. Empty otherwise. Fires at most
    /// once per run.
    pub fn check_before_finalize(&self) -> Vec<LoopMessage> {
        let mut inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if inner.edited_code && !inner.ran_command && !inner.fired {
            inner.fired = true;
            vec![LoopMessage::User(UserMessage {
                content: VERIFY_NUDGE.to_string(),
            })]
        } else {
            Vec::new()
        }
    }
}

/// True if any path argument names a source-code file (by extension).
/// Looks at top-level `path` / `file_path` / `file` and `apply_patch`'s
/// `operations[].path`.
fn touches_code_file(args: &serde_json::Value) -> bool {
    let Some(obj) = args.as_object() else {
        return false;
    };
    let mut paths: Vec<&str> = Vec::new();
    for key in ["path", "file_path", "file"] {
        if let Some(s) = obj.get(key).and_then(|v| v.as_str()) {
            paths.push(s);
        }
    }
    if let Some(ops) = obj.get("operations").and_then(|v| v.as_array()) {
        for op in ops {
            if let Some(s) = op.get("path").and_then(|v| v.as_str()) {
                paths.push(s);
            }
        }
    }
    paths.iter().any(|p| is_code_path(p))
}

/// Source-code file extensions. A change to one of these is "editing
/// code"; docs/config (md, txt, json, toml, …) deliberately don't count,
/// so a doc-only edit never triggers the verify nudge.
const CODE_EXTS: &[&str] = &[
    "rs", "py", "ts", "tsx", "js", "jsx", "mjs", "cjs", "go", "rb", "java", "kt", "kts", "c", "h",
    "cc", "cpp", "hpp", "cxx", "cs", "swift", "php", "scala", "clj", "cljs", "cljc", "ex", "exs",
    "sh", "bash", "lua", "pl", "hs", "ml", "sql", "vue", "svelte",
];

fn is_code_path(path: &str) -> bool {
    match path.rsplit_once('.') {
        Some((_, ext)) => CODE_EXTS.contains(&ext.to_ascii_lowercase().as_str()),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fired(gate: &VerifierGate) -> bool {
        !gate.check_before_finalize().is_empty()
    }

    #[test]
    fn edited_code_without_running_nudges_once() {
        let g = VerifierGate::new();
        g.record_tool_call("edit", &json!({"path": "src/auth.rs"}));
        assert!(fired(&g), "edited code + no command should nudge");
        // Bounded: a second check is silent even if state is unchanged.
        assert!(!fired(&g), "nudge fires at most once per run");
    }

    #[test]
    fn editing_then_running_a_command_stays_silent() {
        let g = VerifierGate::new();
        g.record_tool_call("edit", &json!({"path": "src/auth.rs"}));
        g.record_tool_call("bash", &json!({"command": "cargo test"}));
        assert!(!fired(&g), "running any shell command suppresses the nudge");
    }

    #[test]
    fn doc_only_edit_does_not_nudge() {
        let g = VerifierGate::new();
        g.record_tool_call("write", &json!({"path": "README.md"}));
        g.record_tool_call("edit", &json!({"file_path": "notes.txt"}));
        assert!(!fired(&g), "non-code edits must not count as editing code");
    }

    #[test]
    fn no_edits_never_nudges() {
        let g = VerifierGate::new();
        g.record_tool_call("read", &json!({"path": "src/auth.rs"}));
        g.record_tool_call("grep", &json!({"pattern": "fn main"}));
        assert!(!fired(&g));
    }

    #[test]
    fn apply_patch_with_code_operation_counts_as_edit() {
        let g = VerifierGate::new();
        g.record_tool_call(
            "apply_patch",
            &json!({"operations": [{"type": "update", "path": "src/lib.rs"}]}),
        );
        assert!(fired(&g), "apply_patch touching a code file should count");
    }

    #[test]
    fn apply_patch_doc_only_does_not_count() {
        let g = VerifierGate::new();
        g.record_tool_call(
            "apply_patch",
            &json!({"operations": [{"type": "update", "path": "docs/x.md"}]}),
        );
        assert!(!fired(&g));
    }

    #[test]
    fn is_code_path_recognizes_common_extensions() {
        assert!(is_code_path("src/main.rs"));
        assert!(is_code_path("app/Foo.TS")); // case-insensitive
        assert!(!is_code_path("README.md"));
        assert!(!is_code_path("Makefile")); // no extension
    }
}
