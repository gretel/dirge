//! `/btw <question>` — defer a prompt run with the user's question.

use crate::ui::slash::{SlashCtx, SlashOutcome, c_error};

pub(crate) async fn cmd_btw(
    ctx: &mut SlashCtx<'_>,
    parts: &[&str],
) -> anyhow::Result<SlashOutcome> {
    let query = parts.get(1..).map(|p| p.join(" ")).unwrap_or_default();
    if query.is_empty() {
        ctx.renderer
            .write_line("usage: /btw <question>", c_error())?;
        return Ok(SlashOutcome::Handled);
    }
    Ok(SlashOutcome::DeferBtw { query })
}
