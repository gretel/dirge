//! Native Rust OpenCode provider registrations.
//!
//! Registers provider aliases for the OpenCode API. Uses `provider_type:
//! "openai"` — OpenCode speaks OpenAI-compatible wire protocol, so the
//! existing OpenAI client handles it without any custom HTTP interceptors.
//!
//! Registered aliases:
//! | Name         | Base URL                         | Models                          |
//! |--------------|----------------------------------|---------------------------------|
//! | `opencode`   | https://opencode.ai/zen/v1       | Zen tier (deepseek, claude...)  |
//! | `opencode-go`| https://opencode.ai/zen/go/v1    | Go tier (qwen, glm, mimimax...) |
//!
//! All use `OPENCODE_API_KEY` env var.

use std::collections::HashMap;

use crate::config::ProviderEntry;

/// Returns built-in OpenCode provider entries keyed by short name.
pub fn opencode_providers() -> HashMap<String, ProviderEntry> {
    let mut m = HashMap::new();

    // opencode — OpenAI-compatible, Zen tier
    m.insert(
        "opencode".into(),
        ProviderEntry {
            provider_type: Some("openai".into()),
            base_url: Some("https://opencode.ai/zen/v1".into()),
            model: Some("deepseek-v4-flash".into()),
            auth: None,
            api_key_env: Some("OPENCODE_API_KEY".into()),
            api_key: None,
            allow_insecure: false,
            options: None,
            stream_chunk_timeout_secs: None,
        },
    );

    // opencode-go — OpenAI-compatible, Go tier
    m.insert(
        "opencode-go".into(),
        ProviderEntry {
            provider_type: Some("openai".into()),
            base_url: Some("https://opencode.ai/zen/go/v1".into()),
            model: Some("deepseek-v4-flash".into()),
            auth: None,
            api_key_env: Some("OPENCODE_API_KEY".into()),
            api_key: None,
            allow_insecure: false,
            options: None,
            stream_chunk_timeout_secs: None,
        },
    );

    m
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registers_expected_providers() {
        let p = opencode_providers();
        assert_eq!(p.len(), 2);
        assert!(p.contains_key("opencode"));
        assert!(p.contains_key("opencode-go"));
    }

    #[test]
    fn all_use_opencode_api_key() {
        let p = opencode_providers();
        for (name, entry) in &p {
            assert_eq!(
                entry.api_key_env.as_deref(),
                Some("OPENCODE_API_KEY"),
                "provider {name} should use OPENCODE_API_KEY",
            );
        }
    }

    #[test]
    fn all_use_openai_protocol() {
        let p = opencode_providers();
        for (name, entry) in &p {
            assert_eq!(
                entry.provider_type.as_deref(),
                Some("openai"),
                "provider {name} should use openai protocol",
            );
        }
    }
}
