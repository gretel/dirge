#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_visible_bang() {
        assert_eq!(
            parse_shell_prefix("! ls"),
            Some(ShellPrefix::Visible("ls".into()))
        );
    }

    #[test]
    fn test_invisible_bang() {
        assert_eq!(
            parse_shell_prefix("!! ls"),
            Some(ShellPrefix::Invisible("ls".into()))
        );
    }

    #[test]
    fn test_no_bang() {
        assert_eq!(parse_shell_prefix("ls"), None);
    }

    #[test]
    fn test_bang_without_space() {
        assert_eq!(
            parse_shell_prefix("!ls"),
            Some(ShellPrefix::Visible("ls".into()))
        );
    }

    #[test]
    fn test_double_bang_without_space() {
        assert_eq!(
            parse_shell_prefix("!!ls"),
            Some(ShellPrefix::Invisible("ls".into()))
        );
    }

    #[test]
    fn test_block_cd() {
        assert_eq!(parse_shell_prefix("! cd /tmp"), None);
        assert_eq!(parse_shell_prefix("!! cd /tmp"), None);
        assert_eq!(parse_shell_prefix("!cd /tmp"), None);
    }

    #[test]
    fn test_is_forbidden_skips_env_assignments() {
        assert!(is_forbidden("cd /x"));
        // Leading `VAR=value` must not evade the block.
        assert!(is_forbidden("FOO=1 cd /x"));
        assert!(!is_forbidden("ls"));
        assert!(!is_forbidden("BAR=2 ls"));
    }
}

#[derive(Debug, PartialEq)]
pub enum ShellPrefix {
    Visible(String),
    Invisible(String),
}

pub fn parse_shell_prefix(text: &str) -> Option<ShellPrefix> {
    let trimmed = text.trim();
    if let Some(rest) = trimmed.strip_prefix("!!") {
        let cmd = rest.trim().to_string();
        if cmd.is_empty() || is_forbidden(&cmd) {
            return None;
        }
        return Some(ShellPrefix::Invisible(cmd));
    }
    if let Some(rest) = trimmed.strip_prefix('!') {
        let cmd = rest.trim().to_string();
        if cmd.is_empty() || is_forbidden(&cmd) {
            return None;
        }
        return Some(ShellPrefix::Visible(cmd));
    }
    None
}

fn is_forbidden(cmd: &str) -> bool {
    // Skip leading `name=value` env-assignment tokens so a prefix like
    // `FOO=1 cd /x` is still caught.
    let first = cmd
        .split_whitespace()
        .find(|tok| !is_env_assignment(tok))
        .unwrap_or("");
    matches!(
        first.to_ascii_lowercase().as_str(),
        "cd" | "pushd" | "popd" | "exit" | "exec"
    )
}

/// True for a leading `NAME=value` env-assignment token (NAME is `[A-Za-z_][A-Za-z0-9_]*`).
fn is_env_assignment(tok: &str) -> bool {
    let mut bytes = tok.bytes();
    match bytes.next() {
        Some(b) if b.is_ascii_alphabetic() || b == b'_' => {}
        _ => return false,
    }
    for b in bytes {
        match b {
            b'=' => return true,
            b if b.is_ascii_alphanumeric() || b == b'_' => {}
            _ => return false,
        }
    }
    false
}
