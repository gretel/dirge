use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};

use indexmap::IndexSet;

/// Files the agent has written, edited, or patched in this session, in
/// insertion order (most-recently-modified appears last). The info panel
/// reads this to show a short tail of touched paths so the user has a
/// running record of what the agent has been doing.
///
/// `LazyLock` because `IndexSet::new()` is not `const`. The cost is one
/// extra atomic on first access.
pub static MODIFIED_FILES: LazyLock<Mutex<IndexSet<PathBuf>>> =
    LazyLock::new(|| Mutex::new(IndexSet::new()));

/// Record that `path` was modified by a write/edit/apply_patch tool call.
/// Best-effort canonicalize; falls back to the path as given when the file
/// doesn't exist yet or canonicalize fails.
pub fn mark_modified(path: &Path) {
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut set = MODIFIED_FILES.lock().unwrap_or_else(|e| e.into_inner());
    // IndexSet preserves insertion order and dedups; we want the most-recent
    // touch to surface at the end, so re-insert moves the entry.
    set.shift_remove(&canonical);
    set.insert(canonical);
}

/// Clear the tracked list. Hooked into /clear so the panel resets along
/// with the conversation.
pub fn clear_modified() {
    MODIFIED_FILES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clear();
}

/// Snapshot of the most-recent `n` modified files (newest last). Returns
/// path strings ready for display; entries already canonicalized when
/// possible so the caller can shorten them relative to a working dir.
pub fn recent(n: usize) -> Vec<PathBuf> {
    let set = MODIFIED_FILES.lock().unwrap_or_else(|e| e.into_inner());
    let len = set.len();
    let start = len.saturating_sub(n);
    set.iter().skip(start).cloned().collect()
}
