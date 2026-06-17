//! Provider client construction.
//!
//! Contains `create_client` — the 8-backend dispatch that builds
//! rig clients (OpenAI, Anthropic, Gemini, DeepSeek, GLM, Ollama,
//! OpenRouter, Custom). Extracted from `provider/mod.rs` to keep
//! the provider module focused on type definitions + agent
//! construction.

use std::collections::HashMap;

use rig::http_client::HeaderMap;
use rig::providers::{anthropic, gemini, ollama, openai, openrouter};

use crate::config::{ProviderAuth, ProviderEntry};

use super::auth::{ProviderAuthHeaders, resolve_auth_headers};
use super::codex_http::CodexHttpClient;
use super::{AnyClient, ProviderKind, resolve_api_key, resolve_provider_info};

const CHATGPT_CODEX_BASE_URL: &str = "https://chatgpt.com/backend-api/codex";

pub(crate) fn create_client(
    provider_name: &str,
    api_key: Option<&str>,
    providers: &HashMap<String, ProviderEntry>,
) -> anyhow::Result<AnyClient> {
    create_client_with_auth(provider_name, api_key, providers, None)
}

pub(crate) fn create_client_with_auth(
    provider_name: &str,
    api_key: Option<&str>,
    providers: &HashMap<String, ProviderEntry>,
    default_auth: Option<ProviderAuth>,
) -> anyhow::Result<AnyClient> {
    create_client_with_resolved_auth(provider_name, api_key, providers, default_auth, None)
}

fn create_client_with_resolved_auth(
    provider_name: &str,
    api_key: Option<&str>,
    providers: &HashMap<String, ProviderEntry>,
    default_auth: Option<ProviderAuth>,
    resolved_auth_headers: Option<ProviderAuthHeaders>,
) -> anyhow::Result<AnyClient> {
    let info = resolve_provider_info(provider_name, providers).ok_or_else(|| {
        anyhow::anyhow!(
            "Unknown provider: {}. Supported providers: openrouter, openai, anthropic, gemini, deepseek, glm, ollama, custom",
            provider_name
        )
    })?;

    let auth = info.auth.or(default_auth).unwrap_or(ProviderAuth::ApiKey);
    let auth_headers = match (auth, resolved_auth_headers) {
        (ProviderAuth::ChatGpt, Some(headers)) => Some(headers),
        _ => resolve_auth_headers(auth)?,
    };
    // Precedence for API-key auth: CLI `--api-key` > `entry.api_key`
    // (literal or `${VAR}`-expanded) > `entry.api_key_env` > default
    // env var for the kind > kind-specific fallback env vars.
    // ChatGPT auth intentionally ignores API-key sources and uses the
    // Codex bearer token as the OpenAI client credential.
    let key = if let Some(headers) = auth_headers.as_ref() {
        headers.bearer_token.clone()
    } else {
        match (api_key, info.api_key_literal.as_deref()) {
            (Some(k), _) if !k.is_empty() => k.to_string(),
            (_, Some(k)) if !k.is_empty() => k.to_string(),
            _ => resolve_api_key(info.kind, info.api_key_env.as_deref(), api_key)?,
        }
    };

    let is_chatgpt_auth = auth == ProviderAuth::ChatGpt;
    if is_chatgpt_auth {
        let has_account_id = auth_headers
            .as_ref()
            .and_then(|headers| headers.chatgpt_account_id.as_deref())
            .map(str::trim)
            .is_some_and(|account_id| !account_id.is_empty());
        if !has_account_id {
            anyhow::bail!(
                "ChatGPT auth requested, but no ChatGPT account id was found. Set CHATGPT_ACCOUNT_ID or run `codex login` so auth.json contains a chatgpt_account_id/account_id."
            );
        }
    }
    let base_url = match info.kind {
        ProviderKind::DeepSeek => Some(
            std::env::var("DEEPSEEK_BASE_URL")
                .unwrap_or_else(|_| "https://api.deepseek.com/v1".to_string()),
        ),
        ProviderKind::Glm => Some(
            std::env::var("GLM_BASE_URL")
                .unwrap_or_else(|_| "https://open.bigmodel.cn/api/coding/paas/v4".to_string()),
        ),
        ProviderKind::Custom => info
            .base_url
            .or_else(|| std::env::var("CUSTOM_BASE_URL").ok()),
        ProviderKind::OpenAI if is_chatgpt_auth => info
            .base_url
            .or_else(|| Some(CHATGPT_CODEX_BASE_URL.to_string())),
        _ => info.base_url,
    };

    match info.kind {
        ProviderKind::OpenAI => {
            if is_chatgpt_auth {
                let mut b = openai::Client::builder()
                    .api_key(&key)
                    .http_client(CodexHttpClient::default());
                if let Some(base_url) = &base_url {
                    b = b.base_url(base_url);
                }
                if let Some(headers) = chatgpt_http_headers(auth_headers.as_ref()) {
                    b = b.http_headers(headers);
                }
                Ok(AnyClient::ChatGptOpenAI(b.build()?))
            } else {
                let mut b = openai::CompletionsClient::builder().api_key(&key);
                if let Some(base_url) = &base_url {
                    b = b.base_url(base_url);
                }
                Ok(AnyClient::OpenAI(b.build()?))
            }
        }
        ProviderKind::Anthropic => {
            let mut b = anthropic::Client::builder().api_key(&key);
            if let Some(base_url) = &base_url {
                b = b.base_url(base_url);
            }
            Ok(AnyClient::Anthropic(b.build()?))
        }
        ProviderKind::Gemini => {
            let mut b = gemini::Client::builder().api_key(&key);
            if let Some(base_url) = &base_url {
                b = b.base_url(base_url);
            }
            Ok(AnyClient::Gemini(b.build()?))
        }
        ProviderKind::DeepSeek => {
            let b = openai::CompletionsClient::builder()
                .api_key(&key)
                .base_url(base_url.as_deref().unwrap_or("https://api.deepseek.com/v1"));
            Ok(AnyClient::DeepSeek(b.build()?))
        }
        ProviderKind::Glm => {
            let b = openai::CompletionsClient::builder().api_key(&key).base_url(
                base_url
                    .as_deref()
                    .unwrap_or("https://open.bigmodel.cn/api/coding/paas/v4"),
            );
            Ok(AnyClient::Glm(b.build()?))
        }
        ProviderKind::Ollama => {
            let key: ollama::OllamaApiKey = key.as_str().into();
            let mut b = ollama::Client::builder().api_key(key);
            if let Some(base_url) = &base_url {
                b = b.base_url(base_url);
            }
            Ok(AnyClient::Ollama(b.build()?))
        }
        ProviderKind::OpenRouter => {
            let mut b = openrouter::Client::builder().api_key(&key);
            if let Some(base_url) = &base_url {
                b = b.base_url(base_url);
            }
            Ok(AnyClient::OpenRouter(b.build()?))
        }
        ProviderKind::Custom => {
            let base_url = base_url.ok_or_else(|| {
                anyhow::anyhow!(
                    "CUSTOM_BASE_URL environment variable must be set for custom provider"
                )
            })?;
            let b = openai::CompletionsClient::builder()
                .api_key(&key)
                .base_url(&base_url);
            Ok(AnyClient::Custom(b.build()?))
        }
    }
}

#[cfg(test)]
fn create_client_with_chatgpt_auth_headers(
    provider_name: &str,
    providers: &HashMap<String, ProviderEntry>,
    headers: ProviderAuthHeaders,
) -> anyhow::Result<AnyClient> {
    create_client_with_resolved_auth(
        provider_name,
        None,
        providers,
        Some(ProviderAuth::ChatGpt),
        Some(headers),
    )
}

fn chatgpt_http_headers(
    auth_headers: Option<&super::auth::ProviderAuthHeaders>,
) -> Option<HeaderMap> {
    let account_id = auth_headers?
        .chatgpt_account_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())?;
    let mut headers = HeaderMap::new();
    let name = http::HeaderName::from_static("chatgpt-account-id");
    let value = http::HeaderValue::from_str(account_id).ok()?;
    headers.insert(name, value);
    Some(headers)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::config::{ProviderAuth, ProviderEntry};

    use super::{
        CHATGPT_CODEX_BASE_URL, create_client_with_auth, create_client_with_chatgpt_auth_headers,
        resolve_provider_info,
    };

    fn test_chatgpt_headers() -> crate::provider::auth::ProviderAuthHeaders {
        crate::provider::auth::ProviderAuthHeaders {
            bearer_token: "test-token".to_string(),
            chatgpt_account_id: Some("acct-test".to_string()),
        }
    }

    #[test]
    fn top_level_auth_can_default_provider_entry_auth() {
        let providers = HashMap::from([(
            "openai".to_string(),
            ProviderEntry {
                model: Some("gpt-5.5".to_string()),
                ..Default::default()
            },
        )]);
        let info = resolve_provider_info("openai", &providers).unwrap();

        assert_eq!(
            info.auth.or(Some(ProviderAuth::ChatGpt)),
            Some(ProviderAuth::ChatGpt)
        );
    }

    #[test]
    fn provider_auth_overrides_top_level_default() {
        let providers = HashMap::from([(
            "openai".to_string(),
            ProviderEntry {
                auth: Some(ProviderAuth::ApiKey),
                ..Default::default()
            },
        )]);
        let info = resolve_provider_info("openai", &providers).unwrap();

        assert_eq!(
            info.auth.or(Some(ProviderAuth::ChatGpt)),
            Some(ProviderAuth::ApiKey)
        );
    }

    #[test]
    fn api_key_openai_uses_chat_completions_client() {
        let providers = HashMap::new();

        let client =
            create_client_with_auth("openai", Some("test-api-key"), &providers, None).unwrap();

        let crate::provider::AnyClient::OpenAI(_) = client else {
            panic!("expected API-key OpenAI to use Chat Completions client");
        };
    }

    #[test]
    fn chatgpt_auth_openai_uses_codex_backend_by_default() {
        let providers = HashMap::new();

        let client =
            create_client_with_chatgpt_auth_headers("openai", &providers, test_chatgpt_headers())
                .unwrap();

        let crate::provider::AnyClient::ChatGptOpenAI(client) = client else {
            panic!("expected ChatGPT OpenAI client");
        };
        assert_eq!(client.base_url(), CHATGPT_CODEX_BASE_URL);
    }

    #[test]
    fn chatgpt_auth_openai_preserves_explicit_base_url() {
        let providers = HashMap::from([(
            "openai".to_string(),
            ProviderEntry {
                base_url: Some("https://proxy.example.com/openai".to_string()),
                ..Default::default()
            },
        )]);

        let client =
            create_client_with_chatgpt_auth_headers("openai", &providers, test_chatgpt_headers())
                .unwrap();

        let crate::provider::AnyClient::ChatGptOpenAI(client) = client else {
            panic!("expected ChatGPT OpenAI client");
        };
        assert_eq!(client.base_url(), "https://proxy.example.com/openai");
    }

    #[test]
    fn chatgpt_auth_requires_account_id() {
        let providers = HashMap::new();

        let result = create_client_with_chatgpt_auth_headers(
            "openai",
            &providers,
            crate::provider::auth::ProviderAuthHeaders {
                bearer_token: "test-token".to_string(),
                chatgpt_account_id: None,
            },
        );
        let err = match result {
            Ok(_) => panic!("expected ChatGPT auth without account id to fail"),
            Err(err) => err.to_string(),
        };

        assert!(err.contains("no ChatGPT account id was found"));
    }
}
