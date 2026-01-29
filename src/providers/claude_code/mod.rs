//! Claude Code Provider
//!
//! 基于 OAuth 认证的 Claude Code 订阅 Provider

pub mod anthropic_spoof;
mod constants;
pub mod oauth;

use crate::providers::claude_code::constants::{
    ANTHROPIC_API_VERSION, BETA_FLAGS_BASE, BETA_FLAGS_EXCLUDE,
};
use crate::providers::config;
use crate::providers::{
    parse_anthropic_usage, AuthConfig, OAuthConfig, Provider, ProviderType, StreamingResponse,
    Usage,
};
use crate::utils::{extract_model, should_disable_tls_verify};
use anyhow::{Context, Result};
use async_trait::async_trait;
use bytes::Bytes;
use futures::{Stream, StreamExt};
use http::{header, HeaderMap, HeaderValue};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::OnceLock;
use tokio::sync::{mpsc, Mutex};

/// Rate limit 窗口信息
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RateLimitWindow {
    /// 状态: allowed, allowed_warning, rejected
    pub status: String,
    /// 重置时间 (Unix timestamp)
    pub reset: u64,
    /// 使用率 (0.0 - 1.0)
    pub utilization: f64,
}

/// Claude Code rate limit 信息
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
        let mut builder = Client::builder()
            .timeout(std::time::Duration::from_secs(API_TIMEOUT_SECS))
            .user_agent(user_agent())
            .pool_max_idle_per_host(10);

        if should_disable_tls_verify() {
            tracing::warn!("TLS certificate verification is DISABLED - for debugging only!");
            builder = builder.danger_accept_invalid_certs(true);
        }

        builder.build().expect("Failed to create Claude API client")
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
        let rate_limit_path = providers_dir.join(format!("{}.ratelimit.json", &name));
        let rate_limit = Self::load_rate_limit_from_file(&rate_limit_path);
        Ok(Self {
            providers_dir,
            name,
            cached_oauth: Mutex::new(None),
            rate_limit: std::sync::RwLock::new(rate_limit),
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
            *guard = info.clone();
        }
        self.persist_rate_limit(&info);
    }

    /// Rate limit 持久化文件路径
    fn rate_limit_path(&self) -> PathBuf {
        self.providers_dir
            .join(format!("{}.ratelimit.json", self.name))
    }

    /// 从文件加载 rate limit 信息（同步，用于初始化）
    fn load_rate_limit_from_file(path: &PathBuf) -> RateLimitInfo {
        match std::fs::read_to_string(path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => RateLimitInfo::default(),
        }
    }

    /// 异步持久化 rate limit 信息到文件
    fn persist_rate_limit(&self, info: &RateLimitInfo) {
        let path = self.rate_limit_path();
        let data = match serde_json::to_string(info) {
            Ok(d) => d,
            Err(e) => {
                tracing::warn!("Failed to serialize rate limit: {e}");
                return;
            }
        };
        tokio::spawn(async move {
            if let Err(e) = tokio::fs::write(&path, data).await {
                tracing::warn!("Failed to persist rate limit to {}: {e}", path.display());
            }
        });
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

    /// 发送请求的公共逻辑，返回 (Response, 是否第三方客户端)
    async fn send_request(
        &self,
        mut request: Value,
        stream: bool,
    ) -> Result<(reqwest::Response, bool)> {
        let access_token = self.get_valid_token().await?;

        // 伪装 system 提示词，检测是否第三方客户端
        let is_third_party = anthropic_spoof::spoof_system(&mut request);
        if is_third_party {
            anthropic_spoof::map_request_tools(&mut request);
        }

        // 先从原始 request 构建 headers（包含透传的 headers）
        let headers = build_headers(&access_token, &request)?;
        // 再处理 body（会移除内部字段）
        let body = Self::ensure_stream_field(request, stream);

        // 构建带有 beta=true 参数的 URL
        let mut url = reqwest::Url::parse(ANTHROPIC_API_URL).context("Invalid API URL")?;
        if !url.query_pairs().any(|(k, _)| k == "beta") {
            url.query_pairs_mut().append_pair("beta", "true");
        }

        let response = get_api_client()
            .post(url)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .context("Failed to send request to Claude API")?;

        // 提取 rate limit 信息（只有成功才更新）
        if response.status().is_success() {
            self.update_rate_limit(response.headers());
        }

        Ok((response, is_third_party))
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
        let (response, is_third_party) = self.send_request(request, false).await?;
        let status = response.status();
        let mut response_json: Value = response
            .json()
            .await
            .context("Failed to parse Claude API response")?;

        if !status.is_success() {
            tracing::error!(
                provider = self.name.as_str(),
                status = status.as_u16(),
                body = %response_json,
                "Claude API error response"
            );
        }

        // 第三方客户端需要还原工具名
        if is_third_party {
            anthropic_spoof::unmap_response_tools(&mut response_json);
        }

        Ok(response_json)
    }

    async fn send_streaming(&self, request: Value) -> Result<StreamingResponse> {
        let model = extract_model(&request);
        let (response, is_third_party) = self.send_request(request, true).await?;
        let status = response.status();

        if !status.is_success() {
            let error_body = response.bytes().await.unwrap_or_default();
            tracing::error!(
                provider = self.name.as_str(),
                status = status.as_u16(),
                body = %String::from_utf8_lossy(&error_body),
                "Claude API streaming error response"
            );
            let stream = Box::new(futures::stream::iter(std::iter::once(
                Ok(error_body) as Result<Bytes, std::io::Error>
            )));
            return Ok(StreamingResponse { stream, status });
        }

        let (tx, rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(STREAM_CHANNEL_BUFFER);
        let byte_stream = response.bytes_stream();
        let provider_name = self.name.clone();

        tokio::spawn(async move {
            relay_stream(byte_stream, tx, &provider_name, &model, is_third_party).await;
        });

        let stream = Box::new(tokio_stream::wrappers::ReceiverStream::new(rx));
        Ok(StreamingResponse { stream, status })
    }

    fn rate_limit_info(&self) -> Option<RateLimitInfo> {
        self.rate_limit.read().ok().map(|guard| guard.clone())
    }
}

fn user_agent() -> String {
    format!(
        "claude-cli/{} (external, cli)",
        constants::get_claude_code_version()
    )
}

/// 合并基础 flags 与透传 flags，生成最终的 anthropic-beta 值
fn build_beta_value(data: &Value) -> String {
    let mut flags: BTreeSet<&str> = BETA_FLAGS_BASE.iter().copied().collect();
    if let Some(passed) = data
        .get("_passthrough_headers")
        .and_then(|h| h.get("anthropic-beta"))
        .and_then(|v| v.as_str())
    {
        flags.extend(
            passed
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty() && !BETA_FLAGS_EXCLUDE.contains(s)),
        );
    }
    flags.into_iter().collect::<Vec<_>>().join(",")
}

fn build_headers(access_token: &str, data: &Value) -> Result<HeaderMap> {
    let mut map = HeaderMap::new();

    // 使用 OAuth Bearer token 进行认证（不使用 x-api-key）
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

    // Claude Code 标识 header
    map.insert("x-app", HeaderValue::from_static("cli"));

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
    is_third_party: bool,
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

                    // 处理 SSE 事件
                    let output_event = if is_third_party {
                        transform_sse_event(event, &mut usage)
                    } else {
                        extract_usage_from_event(event, &mut usage);
                        event.to_string()
                    };

                    let event_with_newlines = format!("{}\n\n", output_event);

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

/// 提取 SSE 事件中的 usage 信息（不修改事件）
fn extract_usage_from_event(event: &str, usage: &mut Usage) {
    for line in event.lines() {
        if let Some(data) = parse_sse_data(line) {
            if let Some(event_type) = data.get("type").and_then(|t| t.as_str()) {
                match event_type {
                    "message_start" => {
                        if let Some(msg) = data.get("message") {
                            if let Ok(parsed_usage) = parse_anthropic_usage(msg) {
                                usage.merge_from(&parsed_usage);
                            }
                        }
                    }
                    "message_delta" => {
                        if let Ok(parsed_usage) = parse_anthropic_usage(&data) {
                            usage.merge_from(&parsed_usage);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

/// 转换 SSE 事件（提取 usage 并还原工具名）
fn transform_sse_event(event: &str, usage: &mut Usage) -> String {
    let mut lines: Vec<String> = Vec::new();

    for line in event.lines() {
        if let Some(mut data) = parse_sse_data(line) {
            if let Some(event_type) = data.get("type").and_then(|t| t.as_str()) {
                match event_type {
                    "message_start" => {
                        if let Some(msg) = data.get("message") {
                            if let Ok(parsed_usage) = parse_anthropic_usage(msg) {
                                usage.merge_from(&parsed_usage);
                            }
                        }
                    }
                    "message_delta" => {
                        if let Ok(parsed_usage) = parse_anthropic_usage(&data) {
                            usage.merge_from(&parsed_usage);
                        }
                    }
                    "content_block_start" => {
                        anthropic_spoof::unmap_stream_tool(&mut data);
                    }
                    _ => {}
                }
            }
            lines.push(format!(
                "data: {}",
                serde_json::to_string(&data).unwrap_or_default()
            ));
        } else {
            lines.push(line.to_string());
        }
    }

    lines.join("\n")
}
