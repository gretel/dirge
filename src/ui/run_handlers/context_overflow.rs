//! `AgentEvent::ContextOverflow` handler extracted from `run_interactive`.
//!
//! Auto-recovery for a context-length error mid-run: persist what streamed so
//! far, then kick off a NON-BLOCKING compaction (dirge-tv3p) — the summarizer
//! LLM runs on a spawned task so the UI stays responsive. The retry (respawn the
//! same prompt against the compacted history) is the `RetryAfterOverflow`
//! continuation the `compaction_phase` select! arm runs after install; it only
//! fires when compaction actually shrank the context AND no side-effecting tools
//! ran on the failed turn.

use compact_str::CompactString;
use tokio::sync::mpsc;

use crate::context::ContextFiles;
use crate::event::AgentEvent;
use crate::provider::AnyAgent;
use crate::ui::agent_io::{persist_turn_to_db, render_agent_stream};
use crate::ui::colors::{c_agent, c_error};
use crate::ui::events::sanitize_output;
use crate::ui::run_handlers::{AgentBuildDeps, RunCtx};
use crate::ui::theme;
use crate::ui::tool_display::close_tool_chamber_if_open;

#[allow(clippy::too_many_arguments)]
pub(crate) async fn handle_context_overflow(
    ctx: &mut RunCtx<'_>,
    prompt: CompactString,
    error: CompactString,
    was_reasoning: &mut bool,
    is_running: &mut bool,
    agent: &mut AnyAgent,
    // Kept for signature stability with the call site; install (which needs the
    // context files) now runs in the `compaction_phase` arm with the loop's own.
    _context: &mut ContextFiles,
    // dirge-4y4l: the ~10 build_agent inputs bundled (see AgentBuildDeps).
    deps: &AgentBuildDeps<'_>,
    agent_rx: &mut Option<mpsc::Receiver<AgentEvent>>,
    agent_abort: &mut Option<tokio::task::JoinHandle<()>>,
    agent_interject: &mut Option<mpsc::Sender<()>>,
    agent_cancel: &mut Option<mpsc::Sender<()>>,
    interjection_queue: &std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<String>>>,
    // dirge-tv3p: the loop installs the spawned compaction here; its
    // `compaction_phase` arm runs the retry after the summary lands.
    compaction_phase: &mut Option<crate::ui::compaction::CompactionPhaseHandle>,
) -> anyhow::Result<()> {
    let client = deps.client;
    // Audit H17: the streaming run hit a context-length error. Auto-compact
    // then re-spawn with the same prompt against the now-compacted history —
    // opencode-style automatic recovery (compaction.ts:477-558) instead of
    // leaving the user stranded at the error. dirge-tv3p: the summarizer now
    // runs off-thread (was a 10-60s freeze); the retry is the arm's continuation.
    *was_reasoning = false;
    close_tool_chamber_if_open(ctx.renderer, ctx.last_tool_name, ctx.tool_chamber_open)?;
    // dirge-ufe0: flush any trailing token the render coalescer skipped (the
    // ContextOverflow event queued behind the final tokens leaves them
    // caught-up-but-unpainted) before the overflow line is written.
    if !ctx.response_buf.is_empty() {
        render_agent_stream(
            ctx.response_buf,
            ctx.response_start_line,
            c_agent(),
            ctx.renderer,
        )?;
    }
    let safe = sanitize_output(&error);
    ctx.renderer
        .write_line(&format!("context overflow: {}", safe), c_error())?;
    // Persist what we have so far (partial response + tool calls) before
    // tearing down the runner.
    persist_turn_to_db(
        ctx.session,
        ctx.last_user_prompt,
        ctx.response_buf,
        ctx.tool_calls_buf,
    );
    // Tear down the current runner before compaction.
    if let Some(h) = agent_abort.take() {
        h.abort();
    }
    *agent_rx = None;
    *agent_interject = None;
    *agent_cancel = None;
    *ctx.agent_line_started = false;
    ctx.response_buf.clear();
    *ctx.response_start_line = None;
    ctx.reasoning_buf.clear();
    *ctx.reasoning_start_line = None;

    // Tool-side-effect safety (Review #2): re-issuing the prompt re-runs any
    // side-effecting tool calls the failed turn already made. We have no direct
    // `had_tool_calls` signal (the runner emitted ContextOverflow without saying
    // whether tools fired); approximate it by `tool_calls_this_run > 0`. Captured
    // BEFORE compaction; the arm only retries when it's false. Reset regardless —
    // the failed run is over.
    let tools_already_ran = *ctx.tool_calls_this_run > 0;
    *ctx.tool_calls_this_run = 0;

    ctx.renderer
        .write_line("▒░ auto-compacting then retrying ░▒", theme::accent())?;

    // dirge-tv3p: decide on-thread, then spawn the summarizer off-thread. The
    // `compaction_phase` arm installs the summary and, on success (Compacted &&
    // !tools_already_ran), retries the prompt against the compacted history.
    match crate::ui::slash::prepare_compaction(
        None,
        false, // forced: auto-compaction stays threshold-gated [dirge-fgtj]
        agent,
        client,
        ctx.renderer,
        ctx.session,
        ctx.cfg,
    ) {
        Ok(crate::ui::slash::CompactionDecision::Ready(req)) => {
            *compaction_phase = Some(crate::ui::compaction::spawn(
                *req,
                crate::ui::compaction::CompactionThen::RetryAfterOverflow {
                    prompt: prompt.to_string(),
                    tools_already_ran,
                },
            ));
            // Stay busy while compaction runs; the arm releases / retries.
        }
        Ok(crate::ui::slash::CompactionDecision::NoOp) => {
            // Compaction decided there's nothing to shrink — retrying would just
            // overflow again. Surface it and drop queued messages (safety).
            ctx.renderer.write_line(
                "auto-compact made no progress; leaving session as-is. Try /compress with stricter instructions, lower keep_recent_tokens, or /clear.",
                c_error(),
            )?;
            *is_running = false;
            drop_queued(ctx, interjection_queue, "compact no-op")?;
        }
        Err(e) => {
            ctx.renderer.write_line(
                &format!("auto-compact failed ({e}); leaving session as-is. Try /compress manually or /clear."),
                c_error(),
            )?;
            *is_running = false;
            drop_queued(ctx, interjection_queue, "compact failure")?;
        }
    }
    Ok(())
}

/// Clear queued interjections (they can't run against an over-full context) and
/// tell the user how many were dropped.
fn drop_queued(
    ctx: &mut RunCtx<'_>,
    interjection_queue: &std::sync::Arc<std::sync::Mutex<std::collections::VecDeque<String>>>,
    reason: &str,
) -> anyhow::Result<()> {
    let dropped = interjection_queue.lock().unwrap().len();
    interjection_queue.lock().unwrap().clear();
    if dropped > 0 {
        ctx.renderer.write_line(
            &format!(
                "{} queued message{} dropped due to {reason}",
                dropped,
                if dropped == 1 { "" } else { "s" }
            ),
            c_error(),
        )?;
    }
    Ok(())
}
