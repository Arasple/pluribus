//! Provider 配置
//!
//! 包含所有 Provider 相关的类型定义和配置持久化逻辑
//! TOML 格式: type + [oauth] 或 [api]

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use tokio::fs;

use crate::utils::unix_timestamp_ms;

/// Provider 类型枚举
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum ProviderType {
    Anthropic,
    OpenAI,
    #[clap(name = "claude-code")]
    ClaudeCode,
    Codex,
}

impl ProviderType {
    pub fn is_anthropic(&self) -> bool {
        matches!(self, ProviderType::Anthropic | ProviderType::ClaudeCode)
    }
}

/// Provider 配置
#[derive(Debug, Clone)]
pub struct ProviderConfig {
    pub name: String,
    pub provider_type: ProviderType,
    pub auth: AuthConfig,
}

/// 认证配置
#[derive(Debug, Clone)]
pub enum AuthConfig {
    OAuth(OAuthConfig),
    Api(ApiConfig),
}

/// OAuth 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthConfig {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: u64,
    #[serde(default)]
    pub scopes: Vec<String>,
}

/// API 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiConfig {
    pub base_url: String,
    pub api_key: String,
}

const TOKEN_REFRESH_THRESHOLD_MS: u64 = 5 * 60 * 1000;

impl OAuthConfig {
    pub fn should_refresh(&self) -> bool {
        unix_timestamp_ms() + TOKEN_REFRESH_THRESHOLD_MS >= self.expires_at
    }
}

/// TOML 文件结构
#[derive(Debug, Deserialize, Serialize)]
struct TomlFile {
    #[serde(rename = "type")]
    provider_type: ProviderType,
    oauth: Option<OAuthConfig>,
    api: Option<ApiConfig>,
}

/// 保存配置到文件
pub async fn save(dir: impl AsRef<Path>, name: &str, config: &ProviderConfig) -> Result<()> {
    let dir = dir.as_ref();
    fs::create_dir_all(dir).await?;

    let (oauth, api) = match &config.auth {
        AuthConfig::OAuth(o) => (Some(o.clone()), None),
        AuthConfig::Api(a) => (None, Some(a.clone())),
    };

    let file = TomlFile {
        provider_type: config.provider_type,
        oauth,
        api,
    };

    let path = dir.join(format!("{}.toml", name));
    let content = toml::to_string_pretty(&file)?;
    fs::write(&path, content).await?;

    tracing::info!("Provider {} saved to {}", name, path.display());
    Ok(())
}

/// 加载单个配置
async fn load(path: impl AsRef<Path>) -> Result<ProviderConfig> {
    let path = path.as_ref();
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .context("Invalid file name")?
        .to_string();

    let content = fs::read_to_string(path).await?;
    let file: TomlFile = toml::from_str(&content)?;

    let auth = if let Some(oauth) = file.oauth {
        AuthConfig::OAuth(oauth)
    } else if let Some(api) = file.api {
        AuthConfig::Api(api)
    } else {
        anyhow::bail!("No [oauth] or [api] section in {}", path.display());
    };

    Ok(ProviderConfig {
        name,
        provider_type: file.provider_type,
        auth,
    })
}

/// 加载目录下所有配置
pub async fn load_all(dir: impl AsRef<Path>) -> Result<Vec<ProviderConfig>> {
    let dir = dir.as_ref();
    if !dir.exists() {
        return Ok(vec![]);
    }

    let mut configs = Vec::new();
    let mut entries = fs::read_dir(dir).await?;

    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "toml") {
            match load(&path).await {
                Ok(cfg) => configs.push(cfg),
                Err(e) => tracing::warn!("Failed to load {}: {}", path.display(), e),
            }
        }
    }

    Ok(configs)
}

/// 根据名称加载配置
pub async fn load_by_name(dir: impl AsRef<Path>, name: &str) -> Result<ProviderConfig> {
    let path = dir.as_ref().join(format!("{}.toml", name));
    load(&path).await
}

/// 更新 OAuth 配置
pub async fn update_oauth(dir: impl AsRef<Path>, name: &str, oauth: &OAuthConfig) -> Result<()> {
    let mut config = load_by_name(&dir, name).await?;
    config.auth = AuthConfig::OAuth(oauth.clone());
    save(dir, name, &config).await
}
