//! Typed payloads for the worktree deferral signals carried by
//! [`crate::ui::slash::SlashOutcome::DeferWtMerge`] /
//! [`crate::ui::slash::SlashOutcome::DeferWtExit`].
//!
//! These used to be packed into a `DEFER_WT_*` sentinel string and parsed
//! back on the consumer side; the typed enum made that round-trip redundant,
//! so the pack/parse machinery is gone and only the field structs remain.

/// The paths a `/wt-merge` handoff needs to carry from the slash command
/// (producer) to the UI event loop (consumer): the branch to merge, the
/// merge target, and the main/worktree working-tree paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WtMerge {
    pub branch: String,
    pub target: String,
    pub main_path: String,
    pub wt_path: String,
}

/// The paths a `/wt-exit` handoff needs to carry: the main and worktree
/// working-tree paths.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WtExit {
    pub main_path: String,
    pub wt_path: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wt_merge_holds_the_fields_the_consumer_reads() {
        // The consumer reads `.branch`, `.target`, `.main_path`, `.wt_path`
        // directly off the struct; this pins the field names the typed
        // transport relies on (the old pack/parse round-trip test is gone,
        // since there is no longer a wire format to round-trip).
        let m = WtMerge {
            branch: "feat".into(),
            target: "main".into(),
            main_path: "/repo".into(),
            wt_path: "/repo/.wt/feat".into(),
        };
        assert_eq!(m.branch, "feat");
        assert_eq!(m.target, "main");
        assert_eq!(m.main_path, "/repo");
        assert_eq!(m.wt_path, "/repo/.wt/feat");
    }

    #[test]
    fn wt_exit_holds_the_fields_the_consumer_reads() {
        let x = WtExit {
            main_path: "/repo".into(),
            wt_path: "/repo/.wt/feat".into(),
        };
        assert_eq!(x.main_path, "/repo");
        assert_eq!(x.wt_path, "/repo/.wt/feat");
    }
}
