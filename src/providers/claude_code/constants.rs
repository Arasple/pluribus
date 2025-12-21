//! Claude Code 配置常量

use anyhow::{Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::Rng;
use sha2::{Digest, Sha256};
use std::sync::OnceLock;

pub const ANTHROPIC_API_URL: &str = "https://api.anthropic.com/v1/messages";
pub const ANTHROPIC_API_VERSION: &str = "2023-06-01";

pub const CLAUDE_CODE_OAUTH_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
pub const CLAUDE_CODE_OAUTH_AUTHORIZE_URL: &str = "https://claude.ai/oauth/authorize";
pub const CLAUDE_CODE_OAUTH_TOKEN_URL: &str = "https://console.anthropic.com/v1/oauth/token";
pub const CLAUDE_CODE_OAUTH_REDIRECT_URI: &str = "urn:ietf:wg:oauth:2.0:oob";

pub const CLAUDE_CODE_OAUTH_SCOPES: &[&str] = &[
    "org:create_api_key",
    "user:profile",
    "user:inference",
    "user:sessions:claude_code",
];

/// Claude Code OAuth 需要的基础 beta flags
pub const BETA_FLAGS_BASE: &[&str] = &[
    "claude-code-20250219",
    "fine-grained-tool-streaming-2025-05-14",
    "interleaved-thinking-2025-05-14",
    "oauth-2025-04-20",
];

/// 需要从用户请求中排除的 beta flags
pub const BETA_FLAGS_EXCLUDE: &[&str] = &[];

static CLAUDE_CODE_VERSION: OnceLock<String> = OnceLock::new();
const CLAUDE_CODE_NPM_REGISTRY_URL: &str = "https://registry.npmjs.org/@anthropic-ai/claude-code";
const CLAUDE_CODE_DEFAULT_VERSION: &str = "2.0.75";

pub async fn init_version() -> Result<()> {
    let version = fetch_latest_version().await.unwrap_or_else(|e| {
        tracing::warn!("Failed to fetch Claude Code version: {}", e);
        CLAUDE_CODE_DEFAULT_VERSION.to_string()
    });

    CLAUDE_CODE_VERSION
        .set(version.clone())
        .map_err(|_| anyhow::anyhow!("Version already initialized"))?;

    tracing::info!("Claude Code version: {}", version);
    Ok(())
}

pub fn get_claude_code_version() -> &'static str {
    CLAUDE_CODE_VERSION
        .get()
        .map(|s| s.as_str())
        .unwrap_or(CLAUDE_CODE_DEFAULT_VERSION)
}

async fn fetch_latest_version() -> Result<String> {
    let response: serde_json::Value = crate::utils::get_shared_client()
        .get(CLAUDE_CODE_NPM_REGISTRY_URL)
        .header("Accept", "application/json")
        .send()
        .await
        .context("Failed to fetch npm registry")?
        .json()
        .await
        .context("Failed to parse npm registry response")?;

    Ok(response["dist-tags"]["latest"]
        .as_str()
        .context("Latest version not found")?
        .to_string())
}

#[derive(Debug, Clone)]
pub struct PkceChallenge {
    pub verifier: String,
    pub challenge: String,
}

impl PkceChallenge {
    pub fn generate() -> Self {
        let random_bytes: [u8; 32] = rand::rng().random();
        let verifier = URL_SAFE_NO_PAD.encode(random_bytes);

        let mut hasher = Sha256::new();
        hasher.update(verifier.as_bytes());
        let hash = hasher.finalize();
        let challenge = URL_SAFE_NO_PAD.encode(hash);

        Self {
            verifier,
            challenge,
        }
    }
}
