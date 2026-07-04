//! `/learn` — defer a prompt run that asks the agent to learn from the
//! current session. Always yields a [`SlashOutcome::DeferPromptRun`].

use crate::ui::slash::{SlashCtx, SlashOutcome};

pub(crate) async fn cmd_learn(
    _ctx: &mut SlashCtx<'_>,
    parts: &[&str],
) -> anyhow::Result<SlashOutcome> {
    let request = parts.get(1..).map(|p| p.join(" ")).unwrap_or_default();
    let prompt = crate::agent::learn::build_learn_prompt(&request);
    Ok(SlashOutcome::DeferPromptRun { prompt })
}
