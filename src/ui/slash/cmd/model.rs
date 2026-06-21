//! /model, /reasoning handlers.

use std::collections::HashMap;

#[allow(unused_imports)]
use crate::sync_util::LockExt;
use compact_str::CompactString;

use crate::config::ProviderEntry;
use crate::ui::slash::{SlashCtx, c_agent, c_error, c_result};

/// Build the sorted list of models the config pins, one per provider that
/// sets a `model`. Each row is `(model, provider-alias, is_active)`, sorted
/// by model then alias for stable output. `current` is the active session
/// model, used to flag the selected row. (issue #492 — `/model` listed only
/// the current model with nothing to switch to.)
fn configured_models(
    providers: &HashMap<String, ProviderEntry>,
    current: &str,
) -> Vec<(String, String, bool)> {
    let mut rows: Vec<(String, String, bool)> = providers
        .iter()
        .filter_map(|(alias, entry)| {
            entry
                .model
                .as_ref()
                .map(|m| (m.clone(), alias.clone(), m == current))
        })
        .collect();
    rows.sort();
    rows
}

pub(crate) async fn cmd_model(ctx: &mut SlashCtx<'_>, parts: &[&str]) -> anyhow::Result<()> {
    if parts.len() < 2 {
        ctx.renderer
            .write_line(&format!("current model: {}", ctx.session.model), c_agent())?;

        // List the models pinned across the configured providers so there's
        // something to switch to, marking the active one (issue #492).
        let providers = ctx.cfg.providers_map();
        let rows = configured_models(&providers, ctx.session.model.as_str());
        if rows.is_empty() {
            ctx.renderer.write_line(
                "no models pinned in `providers` config — /model <id> switches to any model your provider supports",
                c_result(),
            )?;
        } else {
            ctx.renderer.write_line("configured models:", c_agent())?;
            for (model, alias, is_active) in &rows {
                let marker = if *is_active { "* " } else { "  " };
                ctx.renderer
                    .write_line(&format!("{marker}{model}  ·  {alias}"), c_result())?;
            }
            ctx.renderer
                .write_line("usage: /model <id> to switch", c_agent())?;
        }
    } else {
        let new_model = CompactString::new(parts[1].trim());
        let model = ctx.client.completion_model(new_model.to_string());
        *ctx.agent = crate::provider::build_agent(
            model,
            ctx.cli,
            ctx.cfg,
            ctx.context,
            ctx.permission.clone(),
            ctx.ask_tx.clone(),
            ctx.question_tx.clone(),
            ctx.plan_tx.clone(),
            ctx.bg_store.clone(),
            #[cfg(feature = "lsp")]
            ctx.lsp_manager.cloned(),
            ctx.sandbox.clone(),
            #[cfg(feature = "mcp")]
            ctx.mcp_manager,
            #[cfg(feature = "semantic")]
            ctx.semantic_manager,
            Some(ctx.session.id.to_string()),
        )
        .await;
        ctx.session.model = new_model.clone();
        ctx.session.provider = ctx.cli.resolve_provider(ctx.cfg);
        let new_ctx = ctx.cfg.resolve_context_window(new_model.as_str());
        let old_ctx = ctx.session.context_window;
        if new_ctx != old_ctx {
            ctx.session.context_window = new_ctx;
        }
        ctx.renderer
            .write_line(&format!("switched to model: {}", new_model), c_agent())?;
        let reserve = ctx.cfg.resolve_reserve_tokens();
        let budget = new_ctx.saturating_sub(reserve);
        if new_ctx < old_ctx && ctx.session.total_estimated_tokens > budget {
            ctx.renderer.write_line(
                &format!(
                    "warning: session uses ~{}k tokens but new model's context budget is ~{}k. Run /compress before the next prompt or the next turn may overflow.",
                    ctx.session.total_estimated_tokens / 1_000,
                    budget / 1_000,
                ),
                c_error(),
            )?;
        }
    }
    Ok(())
}

pub(crate) async fn cmd_reasoning(ctx: &mut SlashCtx<'_>) -> anyhow::Result<()> {
    *ctx.show_reasoning = !*ctx.show_reasoning;
    ctx.renderer.write_line(
        &format!(
            "reasoning visibility: {}",
            if *ctx.show_reasoning { "on" } else { "off" }
        ),
        c_agent(),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(model: Option<&str>) -> ProviderEntry {
        ProviderEntry {
            model: model.map(str::to_string),
            ..Default::default()
        }
    }

    #[test]
    fn lists_pinned_models_sorted_and_flags_active() {
        let providers = HashMap::from([
            (
                "openrouter".to_string(),
                entry(Some("deepseek/deepseek-v4")),
            ),
            ("anthropic".to_string(), entry(Some("claude-opus-4"))),
            // No model pinned → excluded from the list.
            ("local-vllm".to_string(), entry(None)),
        ]);
        let rows = configured_models(&providers, "claude-opus-4");
        assert_eq!(
            rows,
            vec![
                ("claude-opus-4".to_string(), "anthropic".to_string(), true),
                (
                    "deepseek/deepseek-v4".to_string(),
                    "openrouter".to_string(),
                    false,
                ),
            ],
            "sorted by model; the active one is flagged; model-less providers dropped",
        );
    }

    #[test]
    fn empty_when_no_providers_pin_a_model() {
        let providers = HashMap::from([("local-vllm".to_string(), entry(None))]);
        assert!(configured_models(&providers, "anything").is_empty());
        assert!(configured_models(&HashMap::new(), "anything").is_empty());
    }

    #[test]
    fn same_model_under_two_aliases_flags_both() {
        let providers = HashMap::from([
            ("a".to_string(), entry(Some("m"))),
            ("b".to_string(), entry(Some("m"))),
        ]);
        let rows = configured_models(&providers, "m");
        assert!(rows.iter().all(|(_, _, active)| *active));
        assert_eq!(rows.len(), 2);
    }
}
