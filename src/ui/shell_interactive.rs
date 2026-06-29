//! Interactive `!cmd` / `!!cmd` execution: run the command attached to the
//! user's real terminal via a PTY so interactive workflows (`gh auth login`,
//! full-screen prompts, editors) work, while still capturing output to feed
//! to the agent (`!`) or show live only (`!!`).
//!
//! Mirrors `/sandbox attach` (suspend TUI → spawn on PTY → relay → resume) but
//! for the bang-command path. When there is no controlling terminal, or the
//! sandbox backend doesn't run locally, it transparently falls back to the
//! non-interactive capture path in [`crate::ui::shell_exec`].

use crate::sandbox::{Sandbox, SandboxMode};
use crate::ui::ansi;
use crate::ui::pty_relay::PtyRelay;

/// Run `command` interactively and return its captured (escape-stripped)
/// output.
///
/// Suspend the TUI, attach the command to a PTY connected to `/dev/tty`,
/// relay I/O until it exits (capturing output), then resume the TUI. Falls
/// back to [`shell_exec::run_shell_command`] (capture path) when there is no
/// controlling terminal or the sandbox backend runs remotely (microvm).
pub(crate) async fn run(
    command: &str,
    sandbox: &Sandbox,
    renderer: &mut crate::ui::renderer::Renderer,
    user_tx: &tokio::sync::mpsc::UnboundedSender<crate::event::UserEvent>,
) -> anyhow::Result<String> {
    // Microvm runs commands in a remote VM over SSH; the local PTY relay has
    // no local /dev/tty to attach there. Only the local backends (Off/Bwrap)
    // can run interactively on the user's terminal.
    if !matches!(sandbox.mode, SandboxMode::Off | SandboxMode::Bwrap) {
        return crate::ui::shell_exec::run_shell_command(command, sandbox).await;
    }

    let Some(drained_stdin) = crate::ui::terminal::suspend_tui_for_subprocess(user_tx) else {
        // No controlling terminal — run non-interactively (capture path).
        return crate::ui::shell_exec::run_shell_command(command, sandbox).await;
    };

    let result = run_pty(command, sandbox, &drained_stdin).await;

    // Always restore the TUI, even if the relay errored.
    crate::ui::terminal::resume_tui_after_subprocess(renderer, user_tx);
    result
}

async fn run_pty(command: &str, sandbox: &Sandbox, drained_stdin: &[u8]) -> anyhow::Result<String> {
    let mut cmd = sandbox.command_for_pty(command);
    let mut relay = PtyRelay::spawn(&mut cmd)
        .map_err(|e| anyhow::anyhow!("failed to spawn interactive command: {e}"))?;

    // Forward any keystrokes the user typed while the TUI was suspending.
    if !drained_stdin.is_empty() {
        let _ = relay.write_to_primary(drained_stdin);
    }

    // relay_capturing blocks until the child exits; Ctrl+C reaches the child
    // via the PTY (line discipline raises SIGINT), so the user can interrupt
    // normally. Run it on a blocking thread to avoid stalling the runtime.
    let (status, captured) = tokio::task::spawn_blocking(move || relay.relay_capturing())
        .await
        .map_err(|e| anyhow::anyhow!("interactive relay task failed: {e}"))??;

    let mut text = String::from_utf8_lossy(&captured).into_owned();
    let exit_code = status.code().unwrap_or(-1);
    if exit_code != 0 {
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&format!("Exit code: {exit_code}"));
    }
    Ok(ansi::strip_escapes(&text, ansi::StripPolicy::KEEP_NEWLINE))
}
