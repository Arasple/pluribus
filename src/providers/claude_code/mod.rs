//! Claude Code Provider
//!
//! 基于 OAuth 认证的 Claude Code 订阅 Provider

mod constants;
pub mod oauth;

use crate::providers::claude_code::constants::{ANTHROPIC_API_VERSION, BETA_FLAGS_BASE};
use crate::providers::config;
use crate::providers::{
    parse_anthropic_usage, AuthConfig, OAuthConfig, Provider, ProviderType, StreamingResponse,
    Usage,
};
use crate::utils::extract_model;
use anyhow::{Context, Result};
use async_trait::async_trait;
use bytes::Bytes;
use futures::{Stream, StreamExt};
use http::{header, HeaderMap, HeaderValue};
use reqwest::Client;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::OnceLock;
use tokio::sync::{mpsc, Mutex};

/// Rate limit 窗口信息
#[derive(Debug, Clone, Default, Serialize)]
pub struct RateLimitWindow {
    /// 状态: allowed, allowed_warning, rejected
    pub status: String,
    /// 重置时间 (Unix timestamp)
    pub reset: u64,
    /// 使用率 (0.0 - 1.0)
    pub utilization: f64,
}

/// Claude Code rate limit 信息
#[derive(Debug, Clone, Default, Serialize)]
pub struct RateLimitInfo {
    /// 5 小时窗口
    pub five_hour: RateLimitWindow,
    /// 7 天窗口
    pub seven_day: RateLimitWindow,
    /// 最后更新时间
    pub updated_at: u64,
}

use constants::ANTHROPIC_API_URL;

pub use constants::{get_claude_code_version, init_version};
pub use oauth::perform_oauth_login;

/// 流式响应通道缓冲大小
const STREAM_CHANNEL_BUFFER: usize = 100;

/// API 请求超时（秒）
const API_TIMEOUT_SECS: u64 = 300;

/// 共享的 API 客户端（带 user-agent）
static API_CLIENT: OnceLock<Client> = OnceLock::new();

fn get_api_client() -> &'static Client {
    API_CLIENT.get_or_init(|| {
        Client::builder()
            .timeout(std::time::Duration::from_secs(API_TIMEOUT_SECS))
            .user_agent(user_agent())
            .pool_max_idle_per_host(10)
            .build()
            .expect("Failed to create Claude API client")
    })
}

pub struct ClaudeCodeProvider {
    providers_dir: PathBuf,
    name: String,
    cached_oauth: Mutex<Option<OAuthConfig>>,
    rate_limit: std::sync::RwLock<RateLimitInfo>,
}

impl ClaudeCodeProvider {
    pub fn new(providers_dir: PathBuf, name: String) -> Result<Self> {
        Ok(Self {
            providers_dir,
            name,
            cached_oauth: Mutex::new(None),
            rate_limit: std::sync::RwLock::new(RateLimitInfo::default()),
        })
    }

    /// 从响应头提取并更新 rate limit 信息
    fn update_rate_limit(&self, headers: &HeaderMap) {
        let get_str =
            |name: &str| -> Option<&str> { headers.get(name).and_then(|v| v.to_str().ok()) };

        let get_u64 =
            |name: &str| -> u64 { get_str(name).and_then(|s| s.parse().ok()).unwrap_or(0) };

        let get_f64 =
            |name: &str| -> f64 { get_str(name).and_then(|s| s.parse().ok()).unwrap_or(0.0) };

        let info = RateLimitInfo {
            five_hour: RateLimitWindow {
                status: get_str("anthropic-ratelimit-unified-5h-status")
                    .unwrap_or_default()
                    .to_string(),
                reset: get_u64("anthropic-ratelimit-unified-5h-reset"),
                utilization: get_f64("anthropic-ratelimit-unified-5h-utilization"),
            },
            seven_day: RateLimitWindow {
                status: get_str("anthropic-ratelimit-unified-7d-status")
                    .unwrap_or_default()
                    .to_string(),
                reset: get_u64("anthropic-ratelimit-unified-7d-reset"),
                utilization: get_f64("anthropic-ratelimit-unified-7d-utilization"),
            },
            updated_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        };

        if let Ok(mut guard) = self.rate_limit.write() {
            *guard = info;
        }
    }

    /// 获取有效的 access token，必要时自动刷新
    async fn get_valid_token(&self) -> Result<String> {
        // 检查缓存
        {
            let cached = self.cached_oauth.lock().await;
            if let Some(oauth) = &*cached {
                if !oauth.should_refresh() {
                    return Ok(oauth.access_token.clone());
                }
            }
        }

        // 从文件加载
        let cfg = config::load_by_name(&self.providers_dir, &self.name).await?;
        let mut oauth = match cfg.auth {
            AuthConfig::OAuth(o) => o,
            _ => anyhow::bail!("Provider {} is not OAuth type", self.name),
        };

        // 刷新
        if oauth.should_refresh() {
            tracing::info!("Refreshing token for provider {}", self.name);
            oauth = oauth::refresh_token(&oauth.refresh_token).await?;
            config::update_oauth(&self.providers_dir, &self.name, &oauth).await?;
        }

        // 更新缓存
        let token = oauth.access_token.clone();
        {
            let mut cached = self.cached_oauth.lock().await;
            *cached = Some(oauth);
        }

        Ok(token)
    }

    fn ensure_stream_field(mut request: Value, stream: bool) -> Value {
        if let Some(obj) = request.as_object_mut() {
            obj.insert("stream".to_string(), Value::Bool(stream));
            obj.remove("_passthrough_headers");
        }
        request
    }

    /// 发送请求的公共逻辑
    async fn send_request(&self, request: Value, stream: bool) -> Result<reqwest::Response> {
        let access_token = self.get_valid_token().await?;
        // 先从原始 request 构建 headers（包含透传的 headers）
        let headers = build_headers(&access_token, &request)?;
        // 再处理 body（会移除内部字段）
        let body = Self::ensure_stream_field(request, stream);

        let response = get_api_client()
            .post(ANTHROPIC_API_URL)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .context("Failed to send request to Claude API")?;

        // 提取 rate limit 信息（无论成功与否）
        self.update_rate_limit(response.headers());

        let status = response.status();
        if !status.is_success() {
            let error_body = response.text().await.unwrap_or_default();
            anyhow::bail!("Claude API error {}: {}", status, error_body);
        }

        Ok(response)
    }
}

/// 从 SSE data 行解析 JSON
fn parse_sse_data(line: &str) -> Option<Value> {
    line.strip_prefix("data: ")
        .and_then(|json_str| serde_json::from_str(json_str).ok())
}

#[async_trait]
impl Provider for ClaudeCodeProvider {
    fn name(&self) -> &str {
        &self.name
    }

    fn provider_type(&self) -> ProviderType {
        ProviderType::ClaudeCode
    }

    async fn send_message(&self, request: Value) -> Result<Value> {
        let response = self.send_request(request, false).await?;
        response
            .json()
            .await
            .context("Failed to parse Claude API response")
    }

    async fn send_streaming(&self, request: Value) -> Result<StreamingResponse> {
        let model = extract_model(&request);
        let response = self.send_request(request, true).await?;
        let status = response.status();

        let (tx, rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(STREAM_CHANNEL_BUFFER);
        let byte_stream = response.bytes_stream();
        let provider_name = self.name.clone();

        tokio::spawn(async move {
            relay_stream(byte_stream, tx, &provider_name, &model).await;
        });

        let stream = Box::new(tokio_stream::wrappers::ReceiverStream::new(rx));
        Ok(StreamingResponse { stream, status })
    }

    fn rate_limit_info(&self) -> Option<RateLimitInfo> {
        self.rate_limit.read().ok().map(|guard| guard.clone())
    }
}

fn user_agent() -> String {
    format!("claude-code/{}", constants::get_claude_code_version())
}

/// 合并基础 flags 与透传 flags，生成最终的 anthropic-beta 值
fn build_beta_value(data: &Value) -> String {
    let mut flags: BTreeSet<&str> = BETA_FLAGS_BASE.iter().copied().collect();
    if let Some(passed) = data
        .get("_passthrough_headers")
        .and_then(|h| h.get("anthropic-beta"))
        .and_then(|v| v.as_str())
    {
        flags.extend(passed.split(',').map(str::trim).filter(|s| !s.is_empty()));
    }
    flags.into_iter().collect::<Vec<_>>().join(",")
}

fn build_headers(access_token: &str, data: &Value) -> Result<HeaderMap> {
    let mut map = HeaderMap::new();

    map.insert(
        header::AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", access_token))
            .context("Invalid access token for header")?,
    );
    map.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    map.insert(header::ACCEPT, HeaderValue::from_static("application/json"));

    map.insert(
        "anthropic-version",
        HeaderValue::from_static(ANTHROPIC_API_VERSION),
    );

    map.insert(
        "anthropic-beta",
        HeaderValue::from_str(&build_beta_value(data)).context("Invalid beta flags")?,
    );

    Ok(map)
}

async fn relay_stream(
    upstream: impl Stream<Item = std::result::Result<Bytes, reqwest::Error>>,
    tx: mpsc::Sender<std::result::Result<Bytes, std::io::Error>>,
    provider: &str,
    model: &str,
) {
    let mut buffer = String::new();
    let mut pinned = Box::pin(upstream);
    let mut usage = Usage::default();

    while let Some(chunk_result) = pinned.next().await {
        match chunk_result {
            Ok(chunk) => {
                buffer.push_str(&String::from_utf8_lossy(&chunk));

                while let Some(pos) = buffer.find("\n\n") {
                    let event = &buffer[..pos];
                    let event_with_newlines = format!("{}\n\n", event);

                    // 解析 SSE 事件提取 usage
                    for line in event.lines() {
                        if let Some(data) = parse_sse_data(line) {
                            if let Some(event_type) = data.get("type").and_then(|t| t.as_str()) {
                                match event_type {
                                    "message_start" => {
                                        if let Some(msg) = data.get("message") {
                                            usage.merge_from(&parse_anthropic_usage(msg));
                                        }
                                    }
                                    "message_delta" => {
                                        usage.merge_from(&parse_anthropic_usage(&data));
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }

                    if tx.send(Ok(Bytes::from(event_with_newlines))).await.is_err() {
                        tracing::debug!("client disconnected");
                        return;
                    }

                    buffer = buffer[pos + 2..].to_string();
                }
            }
            Err(e) => {
                tracing::error!("stream error: {e}");
                let error_bytes = Bytes::from(format!("data: {{\"error\": \"{}\"}}\n\n", e));
                let _ = tx.send(Ok(error_bytes)).await;
                break;
            }
        }
    }

    if !buffer.is_empty() {
        let _ = tx.send(Ok(Bytes::from(buffer))).await;
    }

    // 流结束时记录 usage
    tracing::info!(
        provider,
        model,
        input_tokens = usage.input_tokens,
        output_tokens = usage.output_tokens,
        cache_read = usage.cache_read_tokens,
        cache_write = usage.cache_creation_tokens,
        "stream completed"
    );
}
