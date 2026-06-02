//! `/debug` slash command — user-facing DAP debugger controls.
//!
//! Provides direct TUI access to the debugger without going through the
//! AI agent. Subcommands: launch, attach, step, continue, step_over,
//! step_in, step_out, terminate, breakpoint, evaluate, sessions, panel.

use std::time::Duration;

use crate::dap::config::{self, ConnectMode};
use crate::dap::session::{DAP_MANAGER, DapSessionManager};
use crate::dap::types::SourceBreakpoint;
use crate::ui::renderer::PanelMode;
use crate::ui::slash::{SlashCtx, c_agent, c_error, c_result};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Entry point for `/debug <subcommand> [args...]`.
pub(super) async fn cmd_debug(ctx: &mut SlashCtx<'_>, parts: &[&str]) -> anyhow::Result<()> {
    if parts.len() < 2 {
        print_usage(ctx).await?;
        return Ok(());
    }

    let sub = parts[1];
    let args = &parts[2..];

    match sub {
        "launch" => cmd_launch(ctx, args).await?,
        "attach" => cmd_attach(ctx, args).await?,
        "step" | "step_over" => cmd_step_over(ctx).await?,
        "step_in" => cmd_step_in(ctx).await?,
        "step_out" => cmd_step_out(ctx).await?,
        "continue" => cmd_continue(ctx).await?,
        "terminate" | "stop" => cmd_terminate(ctx).await?,
        "breakpoint" | "bp" => cmd_breakpoint(ctx, args).await?,
        "evaluate" | "eval" => cmd_evaluate(ctx, args).await?,
        "sessions" | "status" => cmd_sessions(ctx).await?,
        "panel" => cmd_debug_panel(ctx).await?,
        _ => {
            ctx.renderer
                .write_line(&format!("unknown /debug subcommand: {sub}"), c_error())?;
            print_usage(ctx).await?;
        }
    }
    Ok(())
}

async fn print_usage(ctx: &mut SlashCtx<'_>) -> anyhow::Result<()> {
    ctx.renderer
        .write_line("usage: /debug <subcommand> [args...]", c_agent())?;
    ctx.renderer.write_line(
        "  launch <file> [--adapter <name>]    start debugging a program",
        c_result(),
    )?;
    ctx.renderer.write_line(
        "  attach <pid> [--adapter <name>]     attach to a running process",
        c_result(),
    )?;
    ctx.renderer.write_line(
        "  breakpoint <file> <line>            set a breakpoint",
        c_result(),
    )?;
    ctx.renderer.write_line(
        "  step                                step over current line",
        c_result(),
    )?;
    ctx.renderer.write_line(
        "  step_in                             step into function call",
        c_result(),
    )?;
    ctx.renderer.write_line(
        "  step_out                            step out of current function",
        c_result(),
    )?;
    ctx.renderer.write_line(
        "  continue                            resume execution",
        c_result(),
    )?;
    ctx.renderer.write_line(
        "  evaluate <expression>               evaluate an expression",
        c_result(),
    )?;
    ctx.renderer.write_line(
        "  sessions                            show active debug session",
        c_result(),
    )?;
    ctx.renderer.write_line(
        "  terminate                           end debug session",
        c_result(),
    )?;
    ctx.renderer.write_line(
        "  panel                               toggle debug panel on right",
        c_result(),
    )?;
    Ok(())
}

fn get_manager() -> Option<std::sync::Arc<DapSessionManager>> {
    DAP_MANAGER.lock().ok().and_then(|g| g.clone())
}

// ── launch ──────────────────────────────────────────────────────────

async fn cmd_launch(ctx: &mut SlashCtx<'_>, args: &[&str]) -> anyhow::Result<()> {
    if args.is_empty() {
        ctx.renderer
            .write_line("usage: /debug launch <file> [--adapter <name>]", c_error())?;
        return Ok(());
    }

    let program = args[0];
    let adapter_name = parse_flag(args, "--adapter");

    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let prog_path = std::path::Path::new(program);

    let adapter = if let Some(name) = adapter_name {
        config::resolve_adapter(name)
            .ok_or_else(|| anyhow::anyhow!("adapter not found on PATH: {name}"))?
    } else {
        config::select_launch_adapter(prog_path, &cwd, None).ok_or_else(|| {
            anyhow::anyhow!(
                "no debug adapter found for {program}. Install one (debugpy, gdb, lldb-dap, etc.) \
                     or specify --adapter <name>"
            )
        })?
    };

    if adapter.connect_mode == ConnectMode::Socket {
        ctx.renderer
            .write_line("socket-mode adapters are not yet supported", c_error())?;
        return Ok(());
    }

    let mgr = match get_manager() {
        Some(m) => m,
        None => {
            ctx.renderer.write_line(
                "no debug session manager — start a conversation first",
                c_error(),
            )?;
            return Ok(());
        }
    };

    ctx.renderer.write_line(
        &format!("launching {} with adapter {}...", program, adapter.name),
        c_agent(),
    )?;
    ctx.renderer.write_line(
        "  (launch runs in background — use /debug sessions to check result)",
        c_agent(),
    )?;

    // Auto-show the debug panel so the user sees session state as
    // soon as the adapter reports the initial stop.
    ctx.renderer.set_right_panel_mode(PanelMode::Debug);
    ctx.renderer.render_viewport()?;

    // Spawn the actual launch on a background task so the event loop
    // stays responsive. Otherwise the UI freezes while waiting for
    // the adapter handshake + initial stop event (up to 30 s), and
    // the user has no choice but to Ctrl+Z, which leaves the
    // terminal in raw mode.
    let adapter_name = adapter.name.clone();
    let adapter_cmd = adapter.resolved_command.to_string_lossy().to_string();
    let adapter_args = adapter.args.clone();
    let cwd_str = cwd.to_string_lossy().to_string();
    let program = program.to_string();
    let launch_defaults = adapter.launch_defaults.clone();
    let languages = adapter.languages.clone();

    tokio::spawn(async move {
        let signal = crate::agent::agent_loop::tool::AbortSignal::new();
        match mgr
            .launch(
                &adapter_name,
                &adapter_cmd,
                &adapter_args,
                &cwd_str,
                &program,
                &[],
                Some(true),
                Some(launch_defaults),
                &signal,
                DEFAULT_TIMEOUT,
                languages,
            )
            .await
        {
            Ok(_) => {} // session stored in DAP_MANAGER, visible in debug panel
            Err(e) => {
                let msg = format!("/debug launch failed: {e}");
                tracing::error!("{msg}");
                crate::ui::notifications::notify_send(
                    crate::ui::notifications::Notification::Error(msg),
                );
            }
        }
    });

    Ok(())
}

// ── attach ──────────────────────────────────────────────────────────

async fn cmd_attach(ctx: &mut SlashCtx<'_>, args: &[&str]) -> anyhow::Result<()> {
    if args.is_empty() {
        ctx.renderer
            .write_line("usage: /debug attach <pid> [--adapter <name>]", c_error())?;
        return Ok(());
    }

    let pid: u32 = match args[0].parse() {
        Ok(p) => p,
        Err(_) => {
            ctx.renderer
                .write_line(&format!("invalid pid: {}", args[0]), c_error())?;
            return Ok(());
        }
    };

    let adapter_name = parse_flag(args, "--adapter");
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));

    let adapter = if let Some(name) = adapter_name {
        config::resolve_adapter(name)
            .ok_or_else(|| anyhow::anyhow!("adapter not found on PATH: {name}"))?
    } else {
        config::select_attach_adapter(None, None)
            .ok_or_else(|| anyhow::anyhow!("no debug adapter available for attach"))?
    };

    if adapter.connect_mode == ConnectMode::Socket {
        ctx.renderer
            .write_line("socket-mode adapters are not yet supported", c_error())?;
        return Ok(());
    }

    let mgr = match get_manager() {
        Some(m) => m,
        None => {
            ctx.renderer.write_line(
                "no debug session manager — start a conversation first",
                c_error(),
            )?;
            return Ok(());
        }
    };

    ctx.renderer.write_line(
        &format!("attaching to pid {pid} with adapter {}...", adapter.name),
        c_agent(),
    )?;
    ctx.renderer.write_line(
        "  (attach runs in background — use /debug sessions to check result)",
        c_agent(),
    )?;

    ctx.renderer.set_right_panel_mode(PanelMode::Debug);
    ctx.renderer.render_viewport()?;

    let adapter_name = adapter.name.clone();
    let adapter_cmd = adapter.resolved_command.to_string_lossy().to_string();
    let adapter_args = adapter.args.clone();
    let cwd_str = cwd.to_string_lossy().to_string();
    let attach_defaults = adapter.attach_defaults.clone();
    let languages = adapter.languages.clone();

    tokio::spawn(async move {
        let signal = crate::agent::agent_loop::tool::AbortSignal::new();
        match mgr
            .attach(
                &adapter_name,
                &adapter_cmd,
                &adapter_args,
                &cwd_str,
                Some(pid),
                None,
                None,
                Some(attach_defaults),
                &signal,
                DEFAULT_TIMEOUT,
                languages,
            )
            .await
        {
            Ok(_) => {}
            Err(e) => {
                let msg = format!("/debug attach failed: {e}");
                tracing::error!("{msg}");
                crate::ui::notifications::notify_send(
                    crate::ui::notifications::Notification::Error(msg),
                );
            }
        }
    });

    Ok(())
}

// ── breakpoint ──────────────────────────────────────────────────────

async fn cmd_breakpoint(ctx: &mut SlashCtx<'_>, args: &[&str]) -> anyhow::Result<()> {
    if args.len() < 2 {
        ctx.renderer
            .write_line("usage: /debug breakpoint <file> <line>", c_error())?;
        return Ok(());
    }

    let file = args[0];
    let line: u32 = match args[1].parse() {
        Ok(l) => l,
        Err(_) => {
            ctx.renderer
                .write_line(&format!("invalid line number: {}", args[1]), c_error())?;
            return Ok(());
        }
    };

    let mgr = match get_manager() {
        Some(m) => m,
        None => {
            ctx.renderer.write_line(
                "no debug session manager — start a conversation first",
                c_error(),
            )?;
            return Ok(());
        }
    };

    let bp = SourceBreakpoint {
        line: line as i64,
        ..Default::default()
    };

    match mgr.set_breakpoints(file, vec![bp], DEFAULT_TIMEOUT).await {
        Ok(results) => {
            ctx.renderer.write_line(
                &format!("set {} breakpoint(s) in {file}", results.len()),
                c_result(),
            )?;
            for r in &results {
                ctx.renderer.write_line(
                    &format!("  line {} — verified: {}", r.line.unwrap_or(0), r.verified),
                    c_result(),
                )?;
            }
        }
        Err(e) => {
            ctx.renderer
                .write_line(&format!("breakpoint failed: {e}"), c_error())?;
        }
    }
    Ok(())
}

// ── step commands ───────────────────────────────────────────────────

async fn cmd_step_over(ctx: &mut SlashCtx<'_>) -> anyhow::Result<()> {
    let mgr = require_session().await?;
    let signal = crate::agent::agent_loop::tool::AbortSignal::new();
    match mgr.step_over(0, &signal, DEFAULT_TIMEOUT).await {
        Ok(summary) => {
            print_stop(ctx, &summary).await?;
        }
        Err(e) => {
            ctx.renderer
                .write_line(&format!("step failed: {e}"), c_error())?;
        }
    }
    Ok(())
}

async fn cmd_step_in(ctx: &mut SlashCtx<'_>) -> anyhow::Result<()> {
    let mgr = require_session().await?;
    let signal = crate::agent::agent_loop::tool::AbortSignal::new();
    match mgr.step_in(0, &signal, DEFAULT_TIMEOUT).await {
        Ok(summary) => {
            print_stop(ctx, &summary).await?;
        }
        Err(e) => {
            ctx.renderer
                .write_line(&format!("step_in failed: {e}"), c_error())?;
        }
    }
    Ok(())
}

async fn cmd_step_out(ctx: &mut SlashCtx<'_>) -> anyhow::Result<()> {
    let mgr = require_session().await?;
    let signal = crate::agent::agent_loop::tool::AbortSignal::new();
    match mgr.step_out(0, &signal, DEFAULT_TIMEOUT).await {
        Ok(summary) => {
            print_stop(ctx, &summary).await?;
        }
        Err(e) => {
            ctx.renderer
                .write_line(&format!("step_out failed: {e}"), c_error())?;
        }
    }
    Ok(())
}

async fn cmd_continue(ctx: &mut SlashCtx<'_>) -> anyhow::Result<()> {
    let mgr = require_session().await?;
    let signal = crate::agent::agent_loop::tool::AbortSignal::new();
    match mgr.continue_(0, &signal, DEFAULT_TIMEOUT).await {
        Ok(outcome) => {
            ctx.renderer.write_line(
                &format!(
                    "continue → {:?} (stop reason: {})",
                    outcome.status,
                    outcome.stop_reason.as_deref().unwrap_or("none"),
                ),
                c_result(),
            )?;
        }
        Err(e) => {
            ctx.renderer
                .write_line(&format!("continue failed: {e}"), c_error())?;
        }
    }
    Ok(())
}

// ── evaluate ────────────────────────────────────────────────────────

async fn cmd_evaluate(ctx: &mut SlashCtx<'_>, args: &[&str]) -> anyhow::Result<()> {
    if args.is_empty() {
        ctx.renderer
            .write_line("usage: /debug evaluate <expression>", c_error())?;
        return Ok(());
    }
    let expression = args.join(" ");
    let mgr = require_session().await?;
    match mgr.evaluate(&expression, None, None, DEFAULT_TIMEOUT).await {
        Ok(result) => {
            ctx.renderer.write_line(
                &format!(
                    "{expression} = {}",
                    serde_json::to_string_pretty(&result).unwrap_or_else(|_| format!("{result:?}"))
                ),
                c_result(),
            )?;
        }
        Err(e) => {
            ctx.renderer
                .write_line(&format!("evaluate failed: {e}"), c_error())?;
        }
    }
    Ok(())
}

// ── sessions ────────────────────────────────────────────────────────

async fn cmd_sessions(ctx: &mut SlashCtx<'_>) -> anyhow::Result<()> {
    let mgr = match get_manager() {
        Some(m) => m,
        None => {
            ctx.renderer
                .write_line("no debug session manager", c_error())?;
            return Ok(());
        }
    };
    match mgr.active_summary().await {
        Some(s) => {
            ctx.renderer.write_line(
                &format!(
                    "active session: id={} adapter={} status={:?}",
                    s.id, s.adapter_name, s.status,
                ),
                c_result(),
            )?;
            if let Some(reason) = &s.stop_reason {
                ctx.renderer
                    .write_line(&format!("  stop reason: {reason}"), c_result())?;
            }
            if let Some(tid) = s.thread_id {
                ctx.renderer
                    .write_line(&format!("  thread: {tid}"), c_result())?;
            }
        }
        None => {
            ctx.renderer
                .write_line("no active debug session", c_agent())?;
        }
    }
    Ok(())
}

// ── terminate ───────────────────────────────────────────────────────

async fn cmd_terminate(ctx: &mut SlashCtx<'_>) -> anyhow::Result<()> {
    let mgr = require_session().await?;
    match mgr.terminate(DEFAULT_TIMEOUT).await {
        Ok(summary) => {
            ctx.renderer.write_line(
                &format!(
                    "debug session terminated. exit code: {}",
                    summary.exit_code.map_or("none".into(), |c| c.to_string()),
                ),
                c_result(),
            )?;
            let _ = mgr.disconnect(false, DEFAULT_TIMEOUT).await;
        }
        Err(e) => {
            ctx.renderer
                .write_line(&format!("terminate failed: {e}"), c_error())?;
        }
    }
    Ok(())
}

// ── panel ───────────────────────────────────────────────────────────

async fn cmd_debug_panel(ctx: &mut SlashCtx<'_>) -> anyhow::Result<()> {
    ctx.renderer.set_right_panel_mode(PanelMode::Debug);
    ctx.renderer.render_viewport()?;
    ctx.renderer.write_line(
        "debug panel shown on right (use /panel off to hide)",
        c_agent(),
    )?;
    Ok(())
}

// ── helpers ─────────────────────────────────────────────────────────

async fn require_session() -> anyhow::Result<std::sync::Arc<DapSessionManager>> {
    let mgr = match get_manager() {
        Some(m) => m,
        None => {
            anyhow::bail!("no debug session manager — start a conversation first");
        }
    };

    if mgr.active_summary().await.is_none() {
        anyhow::bail!("no active debug session — use /debug launch <file> first");
    }

    Ok(mgr)
}

async fn print_stop(
    ctx: &mut SlashCtx<'_>,
    summary: &crate::dap::types::SessionSummary,
) -> anyhow::Result<()> {
    ctx.renderer.write_line(
        &format!(
            "stopped — reason: {}, thread: {}",
            summary.stop_reason.as_deref().unwrap_or("unknown"),
            summary.thread_id.map_or("?".into(), |t| t.to_string()),
        ),
        c_result(),
    )?;
    Ok(())
}

/// Parse a flag value like `--adapter debugpy` from a positional args slice.
/// Returns `None` if the flag isn't found or if the "value" looks like another
/// flag (starts with `--`), guarding against `--adapter --other-flag` being
/// silently interpreted as adapter `--other-flag`.
fn parse_flag<'a>(args: &[&'a str], flag: &str) -> Option<&'a str> {
    for i in 0..args.len().saturating_sub(1) {
        if args[i] == flag && !args[i + 1].starts_with("--") {
            return Some(args[i + 1]);
        }
    }
    None
}
