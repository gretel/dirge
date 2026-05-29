//! Session allowlist CRUD.
//!
//! Thin free functions operating on `Vec<(String, Pattern)>` that
//! `PermissionChecker` delegates through. Extracted from `checker.rs`
//! so allowlist logic can be unit-tested without instantiating a
//! full permission checker with its configuration wiring.

use crate::permission::engine;
use crate::permission::pattern::Pattern;

#[allow(dead_code)] // Phase 4: legacy session-allowlist match
pub(crate) fn is_allowed(allowlist: &[(String, Pattern)], tool: &str, input: &str) -> bool {
    allowlist
        .iter()
        .any(|(allowed_tool, pattern)| allowed_tool == tool && pattern.matches(input))
}

pub(crate) fn add(allowlist: &mut Vec<(String, Pattern)>, tool: &str, pattern_str: &str) {
    let pattern = engine::pattern_for_tool(tool, pattern_str);
    if allowlist
        .iter()
        .any(|(t, p)| t == tool && p.original == pattern_str)
    {
        return;
    }
    allowlist.push((tool.to_string(), pattern));
}

pub(crate) fn entries(allowlist: &[(String, Pattern)]) -> Vec<(String, String)> {
    allowlist
        .iter()
        .map(|(t, p)| (t.clone(), p.original.clone()))
        .collect()
}

pub(crate) fn remove_at(
    allowlist: &mut Vec<(String, Pattern)>,
    idx: usize,
) -> Option<(String, String)> {
    if idx >= allowlist.len() {
        return None;
    }
    let (tool, pat) = allowlist.remove(idx);
    Some((tool, pat.original.clone()))
}

pub(crate) fn clear(allowlist: &mut Vec<(String, Pattern)>) {
    allowlist.clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_allowlist() -> Vec<(String, Pattern)> {
        let mut al = Vec::new();
        add(&mut al, "bash", "cargo *");
        add(&mut al, "bash", "git *");
        add(&mut al, "read", "/tmp/*");
        al
    }

    #[test]
    fn add_dedupes_identical_entries() {
        let mut al = Vec::new();
        add(&mut al, "bash", "cargo *");
        add(&mut al, "bash", "cargo *");
        add(&mut al, "bash", "cargo *");
        let e = entries(&al);
        assert_eq!(
            e.len(),
            1,
            "expected dedup; got {} entries: {:?}",
            e.len(),
            e
        );
    }

    #[test]
    fn add_keeps_distinct_patterns_for_same_tool() {
        let mut al = Vec::new();
        add(&mut al, "bash", "cargo *");
        add(&mut al, "bash", "git *");
        let e = entries(&al);
        assert_eq!(e.len(), 2, "got: {:?}", e);
    }

    #[test]
    fn clear_empties_the_list() {
        let mut al = make_allowlist();
        assert_eq!(entries(&al).len(), 3);
        clear(&mut al);
        assert!(entries(&al).is_empty());
    }

    #[test]
    fn remove_at_returns_removed_entry() {
        let mut al = make_allowlist();
        assert_eq!(entries(&al).len(), 3);
        let removed = remove_at(&mut al, 1);
        assert_eq!(removed, Some(("bash".to_string(), "git *".to_string())));
        let e = entries(&al);
        assert_eq!(e.len(), 2);
        assert_eq!(e[0], ("bash".to_string(), "cargo *".to_string()));
        assert_eq!(e[1], ("read".to_string(), "/tmp/*".to_string()));
    }

    #[test]
    fn remove_at_out_of_range_returns_none() {
        let mut al = make_allowlist();
        assert_eq!(remove_at(&mut al, 99), None);
        assert_eq!(remove_at(&mut al, 5), None);
        assert_eq!(entries(&al).len(), 3);
    }
}
