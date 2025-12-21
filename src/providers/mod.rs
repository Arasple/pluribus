//! Provider 抽象层
//!
//! 定义所有 AI Provider 的统一接口，从 providers/*.toml 加载配置

pub mod claude_code;
pub mod config;

use anyhow::Result;
use async_trait::async_trait;
use bytes::Bytes;
use futures::Stream;
use serde_json::Value;
use std::path::Path;
use std::sync::Arc;

use claude_code::ClaudeCodeProvider;
pub use claude_code::RateLimitInfo;
pub use config::{save, AuthConfig, OAuthConfig, ProviderConfig, ProviderType};

/// Token 使用统计
#[derive(Debug, Clone, Default)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
}

impl Usage {
    /// 合并另一个 Usage，非零值会覆盖当前值
    pub fn merge_from(&mut self, other: &Usage) {
        if other.input_tokens > 0 {
            self.input_tokens = other.input_tokens;
        }
        if other.output_tokens > 0 {
            self.output_tokens = other.output_tokens;
        }
        if other.cache_read_tokens > 0 {
            self.cache_read_tokens = other.cache_read_tokens;
        }
        if other.cache_creation_tokens > 0 {
            self.cache_creation_tokens = other.cache_creation_tokens;
        }
    }
}

/// 从 Anthropic API 响应中解析 Usage 信息
///
/// # 参数
///
/// * `response` - Anthropic API 的 JSON 响应
///
/// # 返回值
///
/// 返回解析后的 `Usage` 结构，包含各类 token 用量统计
/// 如果任意值为0，返回 `Err`
///
/// # 说明
///
/// 这个函数解析 Anthropic API 响应中的 usage 字段，提取：
/// - input_tokens: 输入 token 数
/// - output_tokens: 输出 token 数
/// - cache_read_input_tokens: 缓存读取的 token 数
/// - cache_creation_input_tokens: 缓存创建的 token 数
pub fn parse_anthropic_usage(response: &Value) -> Result<Usage> {
    let usage_obj = response
        .get("usage")
        .ok_or_else(|| anyhow::anyhow!("Missing usage field"))?;

    let input_tokens = usage_obj
        .get("input_tokens")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| anyhow::anyhow!("Missing or invalid input_tokens"))?;

    let output_tokens = usage_obj
        .get("output_tokens")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| anyhow::anyhow!("Missing or invalid output_tokens"))?;

    let cache_read_tokens = usage_obj
        .get("cache_read_input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let cache_creation_tokens = usage_obj
        .get("cache_creation_input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    // 如果任意值为0，返回错误
    if input_tokens == 0
        || output_tokens == 0
        || cache_read_tokens == 0
        || cache_creation_tokens == 0
    {
        return Err(anyhow::anyhow!("Usage contains zero values"));
    }

    Ok(Usage {
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_creation_tokens,
    })
}

/// 流式响应
pub struct StreamingResponse {
    pub stream: Box<dyn Stream<Item = Result<Bytes, std::io::Error>> + Send + Unpin>,
    pub status: http::StatusCode,
}

/// Provider Trait - 所有 AI 服务提供商的统一接口
#[async_trait]
pub trait Provider: Send + Sync {
    /// Provider 名称（用于日志和标识）
    fn name(&self) -> &str;
    fn provider_type(&self) -> ProviderType;
    async fn send_message(&self, request: Value) -> Result<Value>;
    async fn send_streaming(&self, request: Value) -> Result<StreamingResponse>;

    /// 获取 rate limit 信息（仅部分 provider 支持）
    fn rate_limit_info(&self) -> Option<RateLimitInfo> {
        None
    }
}

/// 从 providers 目录加载所有 Provider
pub async fn load_providers(providers_dir: impl AsRef<Path>) -> Result<Vec<Arc<dyn Provider>>> {
    let providers_dir = providers_dir.as_ref();
    let configs = config::load_all(providers_dir).await?;

    if configs.is_empty() {
        tracing::warn!("No providers found. Run 'pluribus login claude-code' to add one.");
        return Ok(vec![]);
    }

    tracing::info!("Loaded {} provider config(s)", configs.len());

    let mut providers: Vec<Arc<dyn Provider>> = Vec::new();

    for cfg in configs {
        match create_provider(providers_dir, cfg) {
            Ok(provider) => providers.push(provider),
            Err(e) => tracing::warn!("Failed to create provider: {}", e),
        }
    }

    Ok(providers)
}

/// 根据配置创建 Provider
fn create_provider(providers_dir: &Path, config: ProviderConfig) -> Result<Arc<dyn Provider>> {
    match config.provider_type {
        ProviderType::ClaudeCode => {
            let provider = ClaudeCodeProvider::new(providers_dir.to_path_buf(), config.name)?;
            Ok(Arc::new(provider))
        }
        other => anyhow::bail!("Unknown provider type: {other:?}"),
    }
}
