//! Model-family detection for model-aware harness steering.
//!
//! The harness ships byte-identical guidance to every model today. Some
//! steering only makes sense for specific model families — e.g. DeepSeek
//! benefits from structural-constraint framing and explicit anti-drift
//! rules, and a DeepSeek **reasoner** (R1) ignores system prompts and has
//! weak tool-calling, so the harness must know what it's talking to.
//!
//! This module is the single source of truth for that classification:
//! a pure `resolve_family(provider, model_id)` so the preamble assembler
//! and request builder can branch without scattering string-matching.

/// Which vendor produced the model. Only DeepSeek is special-cased today;
/// everything else is `Other` (no model-specific steering applied).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vendor {
    Deepseek,
    Other,
}

/// Whether the model is a standard chat model or a dedicated reasoner.
/// DeepSeek's `deepseek-reasoner` (R1) is a reasoner; `deepseek-v3/v4`
/// are chat models.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelKind {
    Chat,
    Reasoner,
}

/// Resolved capabilities + identity used to steer prompting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelFamily {
    pub vendor: Vendor,
    pub kind: ModelKind,
    /// False for DeepSeek R1, which ignores a system prompt — the
    /// preamble must be relocated into the first user turn for it.
    pub supports_system_prompt: bool,
    /// False for DeepSeek R1, which lacks reliable function-calling.
    pub supports_tools: bool,
}

impl ModelFamily {
    /// Convenience: a DeepSeek chat model (v3/v4 family), the target of
    /// the gated guidance fragment.
    pub fn is_deepseek_chat(&self) -> bool {
        self.vendor == Vendor::Deepseek && self.kind == ModelKind::Chat
    }
}

/// Classify a `(provider, model_id)` pair into a [`ModelFamily`].
///
/// `provider` is the dirge provider alias (e.g. `"deepseek"`,
/// `"openrouter"`, `"openai"`); `model_id` is the concrete model string
/// (e.g. `"deepseek-v4-pro"`, `"deepseek/deepseek-reasoner"`). Matching
/// is case-insensitive. DeepSeek is detected either by provider name or
/// by a `deepseek` marker in the model id (so an OpenRouter passthrough
/// like `deepseek/deepseek-chat` still classifies correctly).
pub fn resolve_family(provider: &str, model_id: &str) -> ModelFamily {
    let provider = provider.to_ascii_lowercase();
    let model = model_id.to_ascii_lowercase();

    let vendor = if provider == "deepseek" || model.contains("deepseek") {
        Vendor::Deepseek
    } else {
        Vendor::Other
    };

    let kind = if model.contains("reasoner") || has_r1_token(&model) {
        ModelKind::Reasoner
    } else {
        ModelKind::Chat
    };

    // Only DeepSeek's reasoner (R1) is crippled — it ignores the system
    // prompt and lacks reliable function-calling. Other vendors' reasoners
    // (e.g. OpenAI o-series) keep both.
    let crippled_reasoner = vendor == Vendor::Deepseek && kind == ModelKind::Reasoner;

    ModelFamily {
        vendor,
        kind,
        supports_system_prompt: !crippled_reasoner,
        supports_tools: !crippled_reasoner,
    }
}

/// True if `model` contains a standalone `r1` token (bounded by
/// non-alphanumeric separators or string ends), so `deepseek-r1` and
/// `deepseek/r1-0528` match but `super1-model` does not.
fn has_r1_token(model: &str) -> bool {
    model
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|tok| tok == "r1")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deepseek_chat_v4_is_chat_with_full_capabilities() {
        let f = resolve_family("deepseek", "deepseek-v4-pro");
        assert_eq!(f.vendor, Vendor::Deepseek);
        assert_eq!(f.kind, ModelKind::Chat);
        assert!(f.supports_system_prompt);
        assert!(f.supports_tools);
        assert!(f.is_deepseek_chat());
    }

    #[test]
    fn deepseek_reasoner_loses_system_prompt_and_tools() {
        let f = resolve_family("deepseek", "deepseek-reasoner");
        assert_eq!(f.vendor, Vendor::Deepseek);
        assert_eq!(f.kind, ModelKind::Reasoner);
        assert!(!f.supports_system_prompt);
        assert!(!f.supports_tools);
        assert!(!f.is_deepseek_chat());
    }

    #[test]
    fn deepseek_r1_id_is_detected_as_reasoner() {
        let f = resolve_family("deepseek", "deepseek-r1");
        assert_eq!(f.vendor, Vendor::Deepseek);
        assert_eq!(f.kind, ModelKind::Reasoner);
    }

    #[test]
    fn openrouter_passthrough_detected_by_model_id() {
        let f = resolve_family("openrouter", "deepseek/deepseek-chat");
        assert_eq!(f.vendor, Vendor::Deepseek);
        assert_eq!(f.kind, ModelKind::Chat);
        assert!(f.is_deepseek_chat());
    }

    #[test]
    fn openrouter_passthrough_reasoner_detected() {
        let f = resolve_family("openrouter", "deepseek/deepseek-r1");
        assert_eq!(f.vendor, Vendor::Deepseek);
        assert_eq!(f.kind, ModelKind::Reasoner);
        assert!(!f.supports_tools);
    }

    #[test]
    fn case_insensitive() {
        let f = resolve_family("DeepSeek", "DEEPSEEK-V4-FLASH");
        assert_eq!(f.vendor, Vendor::Deepseek);
        assert_eq!(f.kind, ModelKind::Chat);
    }

    #[test]
    fn openai_is_other_vendor_chat() {
        let f = resolve_family("openai", "gpt-4o");
        assert_eq!(f.vendor, Vendor::Other);
        assert_eq!(f.kind, ModelKind::Chat);
        assert!(f.supports_system_prompt);
        assert!(f.supports_tools);
        assert!(!f.is_deepseek_chat());
    }

    #[test]
    fn other_vendor_reasoner_keeps_system_prompt_and_tools() {
        // Non-DeepSeek reasoners (e.g. OpenAI o-series) DO support
        // system/developer prompts and tools — only DeepSeek R1 is
        // crippled. Detection still flags the reasoner kind.
        let f = resolve_family("openai", "o3-reasoner");
        assert_eq!(f.vendor, Vendor::Other);
        assert_eq!(f.kind, ModelKind::Reasoner);
        assert!(f.supports_system_prompt);
        assert!(f.supports_tools);
    }

    #[test]
    fn anthropic_is_other_chat() {
        let f = resolve_family("anthropic", "claude-sonnet-4-6");
        assert_eq!(f.vendor, Vendor::Other);
        assert_eq!(f.kind, ModelKind::Chat);
    }

    #[test]
    fn r1_substring_does_not_false_positive_mid_token() {
        // A model id that merely contains the letters "r1" inside a
        // larger token (e.g. "gpt-4-turbo1") must not be misread as a
        // reasoner — only a standalone r1 token counts.
        let f = resolve_family("openai", "super1-model");
        assert_eq!(f.kind, ModelKind::Chat);
    }
}
