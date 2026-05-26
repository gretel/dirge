//! Path resolution helpers for the permission checker.
//!
//! Provides canonicalisation, symlink resolution, and
//! builtin-allow rule installation (CWD-scoped write/edit/apply_patch
//! and /dev/null exemption). Extracted from `checker.rs`.
//!
//! `resolve_absolute` is the public entry point — resolves a
//! possibly-relative path through the working directory, follows
//! symlinks, and normalises `..` / `.` components. Used by
//! `PermissionChecker` and by external callers that need the same
//! canonical path the permission check ran against (closing the
//! symlink-swap TOCTOU between check and open).

use std::collections::HashMap;
use std::path::Path;

use crate::permission::Action;
use crate::permission::engine;
use crate::permission::pattern::Pattern;

/// One-shot canonicalize for the working-directory cache. Best
/// effort: if canonicalize fails (cwd doesn't exist on disk, e.g.
/// in tests that pass a fixture path), fall back to the literal
/// string so the `starts_with` comparisons in `is_external_path`
/// still work for the literal form.
pub(crate) fn canonicalize_for_cache(working_dir: &str) -> String {
    std::fs::canonicalize(working_dir)
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| working_dir.to_string())
}

/// Install the CWD-scoped builtin-allow rule on `rules` for the
/// mutating filesystem tools (write/edit/apply_patch). Returns the
/// pattern string installed (`Some`) so `set_working_dir` can find
/// and remove it on cd; `None` when the working_dir is too
/// degenerate to install safely.
///
/// Refuses to install when:
///   - `working_dir` is empty (config-only init w/o cwd resolution).
///   - The canonical form is `/` or shorter than 2 chars — the
///     resulting pattern (`/**`) would silently allow writes anywhere
///     on the filesystem, defeating the "permissive only inside the
///     project" intent.
///   - `working_dir` contains glob metacharacters (`*`, `?`, `[`,
///     `{`). Such characters would be re-interpreted by the glob
///     compiler rather than matched literally; a user starting dirge
///     from `/tmp/[odd]` would get a character-class pattern matching
///     unintended paths.
///
/// Uses `canonicalize_for_cache` so the pattern matches the canonical
/// form `resolve_absolute` produces. Without this, macOS users whose
/// `/var` / `/tmp` resolve to `/private/var` / `/private/tmp` would
/// see the rule silently fail to match for any abs_path the checker
/// computed.
pub(crate) fn install_cwd_allow_rules(
    rules: &mut HashMap<String, Vec<(Pattern, Action)>>,
    working_dir: &str,
) -> Option<String> {
    if working_dir.is_empty() {
        return None;
    }
    let canonical = canonicalize_for_cache(working_dir);
    let trimmed = canonical.trim_end_matches('/');
    if trimmed.is_empty() || trimmed == "/" || canonical.len() < 2 {
        return None;
    }
    if trimmed.chars().any(|c| matches!(c, '*' | '?' | '[' | '{')) {
        return None;
    }
    let cwd_glob = format!("{}/**", trimmed);
    for tool in ["write", "edit", "apply_patch"] {
        rules
            .entry(tool.to_string())
            .or_default()
            .push((engine::pattern_for_tool(tool, &cwd_glob), Action::Allow));
    }
    Some(cwd_glob)
}

/// Install a builtin-allow for `/dev/null` on every tool so the
/// harmless bit-bucket never triggers a permission prompt. Writes
/// to `/dev/null` discard data; reads return immediate EOF — no
/// side effects, no security risk, no reason to ask.
pub(crate) fn install_dev_null_allow(rules: &mut HashMap<String, Vec<(Pattern, Action)>>) {
    for tool in [
        "read",
        "write",
        "edit",
        "apply_patch",
        "glob",
        "grep",
        "find_files",
        "list_dir",
        "list_symbols",
        "find_definition",
        "find_callers",
        "find_callees",
        "get_symbol_body",
        "repo_overview",
        "lsp",
    ] {
        rules
            .entry(tool.to_string())
            .or_default()
            .push((engine::pattern_for_tool(tool, "/dev/null"), Action::Allow));
    }
}

pub(crate) fn resolve_absolute(path: &str, working_dir: &str) -> String {
    let p = Path::new(path);
    let joined = if p.is_absolute() {
        p.to_path_buf()
    } else {
        Path::new(working_dir).join(p)
    };
    match std::fs::canonicalize(&joined) {
        Ok(canonical) => canonical.to_string_lossy().to_string(),
        Err(_) => {
            if let (Some(parent), Some(name)) = (joined.parent(), joined.file_name())
                && let Ok(canonical_parent) = std::fs::canonicalize(parent)
            {
                return canonical_parent.join(name).to_string_lossy().to_string();
            }
            lexical_normalize(&joined).to_string_lossy().to_string()
        }
    }
}

/// Resolve `.` and `..` components of `p` without touching the
/// filesystem. `..` pops the previous `Normal` component; consecutive
/// `..` at the start (i.e. attempting to climb above root) are
/// retained as `..` so an attacker can't disguise an escape by
/// chaining enough `..` to underflow a real-path prefix check.
/// Doesn't follow symlinks — callers that need symlink resolution
/// should use `std::fs::canonicalize`; this helper exists for the
/// nonexistent-path fallback where canonicalize is impossible.
fn lexical_normalize(p: &Path) -> std::path::PathBuf {
    use std::path::{Component, PathBuf};
    let mut out: Vec<Component> = Vec::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if matches!(out.last(), Some(Component::Normal(_))) {
                    out.pop();
                } else {
                    out.push(c);
                }
            }
            other => out.push(other),
        }
    }
    let mut buf = PathBuf::new();
    for c in &out {
        buf.push(c.as_os_str());
    }
    buf
}

/// Reject paths that are clearly LLM hallucinations before they
/// trigger permission dialogs.  Relative single-segment paths
/// that are purely numeric ("1", "42") or trivially short
/// ("a", "x") are never valid file names a well-behaved
/// agent would genuinely want to use; the model is confusing
/// a counter, index, or file-descriptor number with a file
/// path.
///
/// Returns `Ok(())` for plausible paths, `Err(reason)` for
/// paths that should be hard-rejected.
pub fn validate_path(path: &str) -> Result<(), String> {
    let p = Path::new(path);
    if p.is_absolute() {
        return Ok(());
    }
    // Has a directory component — plausible relative path.
    if path.contains('/') || path.contains('\\') {
        return Ok(());
    }
    // Has a file extension — plausible filename.
    if path.contains('.') {
        return Ok(());
    }
    // Just a bare name.  Reject single-segment names that are
    // purely numeric ("1", "42") or a single short token
    // with no extension ("a", "xy").
    if path.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!(
            "Refusing to use numeric path {:?}. Use an absolute path with a real file name.",
            path,
        ));
    }
    if path.chars().count() <= 2 {
        return Err(format!(
            "Refusing to use trivial path {:?}. Use an absolute path with a real file name.",
            path,
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// F7: `resolve_absolute` must follow symlinks so a symlink
    /// pointing at a deny-listed path can't bypass the rule.
    #[test]
    fn resolve_absolute_follows_symlinks() {
        let dir =
            std::env::temp_dir().join(format!("dirge-f7-symlink-test-{}", std::process::id(),));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("real-secret.txt");
        std::fs::write(&target, "hunter2").unwrap();
        let link = dir.join("benign-name.txt");

        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &link).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&target, &link).unwrap();

        let resolved = resolve_absolute(link.to_str().unwrap(), "/");
        let expected = std::fs::canonicalize(&target)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert_eq!(resolved, expected, "symlink should resolve to its target",);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// F7: nonexistent paths (writes to new files) must still
    /// resolve sensibly. They can't canonicalize fully but we
    /// canonicalize the parent so `/real/parent/../../etc/passwd`
    /// becomes `/etc/passwd` instead of staying lexical.
    #[test]
    fn resolve_absolute_handles_nonexistent_via_parent_canonicalize() {
        let dir =
            std::env::temp_dir().join(format!("dirge-f7-newfile-test-{}", std::process::id(),));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let new_file = dir.join("does-not-exist-yet.txt");

        let resolved = resolve_absolute(new_file.to_str().unwrap(), "/");
        let expected_parent = std::fs::canonicalize(&dir).unwrap();
        let expected = expected_parent
            .join("does-not-exist-yet.txt")
            .to_string_lossy()
            .into_owned();
        assert_eq!(resolved, expected);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Regression for audit C3: when BOTH `canonicalize(joined)` AND
    /// `canonicalize(parent)` fail, the previous fallback returned
    /// the joined path with `..` components intact. Since
    /// `Path::starts_with` operates on path *components*, a crafted
    /// path like `/cwd/nonexistent_subdir/../../etc/passwd` would
    /// classify as internal because the first three components match
    /// `/cwd`. Attacker (LLM/agent) can synthesize such a path
    /// trivially. After the fix, `..` components are lexically
    /// resolved before the fallback returns, so the path escapes
    /// the cwd subtree.
    #[test]
    fn resolve_absolute_normalizes_dotdot_in_full_lexical_fallback() {
        let dir = std::env::temp_dir().join(format!("dirge-c3-traversal-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let cwd = dir.to_string_lossy().into_owned();

        let traversal = "no_such_dir/no_such_subdir/../../../etc/passwd";

        let resolved = resolve_absolute(traversal, &cwd);

        let cwd_canonical = std::fs::canonicalize(&cwd).unwrap();
        let resolved_path = std::path::PathBuf::from(&resolved);
        assert!(
            !resolved_path.starts_with(&cwd_canonical) && !resolved_path.starts_with(&cwd),
            "lexical-fallback path-traversal should escape cwd subtree; got {:?}, cwd {:?}",
            resolved_path,
            cwd_canonical,
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── validate_path ────────────────────────────────────────────

    #[test]
    fn validate_accepts_absolute_paths() {
        assert!(validate_path("/etc/hosts").is_ok());
        assert!(validate_path("/Users/bob/src/main.rs").is_ok());
    }

    #[test]
    fn validate_accepts_relative_paths_with_separator() {
        assert!(validate_path("src/main.rs").is_ok());
        assert!(validate_path("lib/core.js").is_ok());
        assert!(validate_path("..\\windows\\path").is_ok());
    }

    #[test]
    fn validate_accepts_relative_names_with_extension() {
        assert!(validate_path("Cargo.toml").is_ok());
        assert!(validate_path("README.md").is_ok());
        assert!(validate_path("build.sh").is_ok());
    }

    #[test]
    fn validate_accepts_extensionless_names_that_are_not_trivial() {
        // Common extensionless filenames.
        assert!(validate_path("Makefile").is_ok());
        assert!(validate_path("Dockerfile").is_ok());
        assert!(validate_path("README").is_ok());
        assert!(validate_path("LICENSE").is_ok());
        assert!(validate_path("abc").is_ok());
    }

    #[test]
    fn validate_rejects_numeric_paths() {
        assert!(validate_path("1").is_err());
        assert!(validate_path("42").is_err());
        assert!(validate_path("007").is_err());
    }

    #[test]
    fn validate_rejects_short_nonsense_paths() {
        assert!(validate_path("a").is_err());
        assert!(validate_path("xy").is_err());
    }
}
