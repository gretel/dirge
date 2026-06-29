//! Whether a `!`/`!!` bang command feeds its captured output to the agent
//! (`Visible`) or shows it live only (`Invisible`). The execution itself lives
//! in [`crate::ui::shell_interactive`] (PTY-attached, interactive-capable).

/// Whether the command's output is fed to the agent as a new turn (`Visible`)
/// or merely shown live on the terminal (`Invisible`).
pub(crate) enum ShellKind {
    Visible,
    Invisible,
}
