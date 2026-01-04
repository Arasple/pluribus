//! OAuth 客户端和登录流程

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::io::{self, Write};
use std::path::PathBuf;

use crate::providers::OAuthConfig;
use crate::utils::unix_timestamp_ms;

use super::constants::{
    generate_random_base64url, PkceChallenge, CLAUDE_CODE_OAUTH_AUTHORIZE_URL,
    CLAUDE_CODE_OAUTH_CLIENT_ID, CLAUDE_CODE_OAUTH_REDIRECT_URI, CLAUDE_CODE_OAUTH_SCOPES,
    CLAUDE_CODE_OAUTH_TOKEN_URL,
};

/// 登录会话缓存，用于调试时复用同一个授权 URL
#[derive(Debug, Clone, Serialize, Deserialize)]
struct OAuthLoginCache {
    /// PKCE verifier
    verifier: String,
    /// PKCE challenge
    challenge: String,
    /// OAuth state (独立于 verifier)
    state: String,
    /// 完整的授权 URL
    authorize_url: String,
    /// 缓存创建时间
    created_at: u64,
}

impl OAuthLoginCache {
    /// 缓存有效期：1 小时
    const CACHE_TTL_MS: u64 = 3600 * 1000;

    fn cache_path() -> PathBuf {
        let cache_dir = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("pluribus");
        std::fs::create_dir_all(&cache_dir).ok();
        cache_dir.join("oauth_login_cache.json")
    }

    fn load() -> Option<Self> {
        let path = Self::cache_path();
        let content = std::fs::read_to_string(&path).ok()?;
        let cache: Self = serde_json::from_str(&content).ok()?;

        // 检查缓存是否过期
        let now = unix_timestamp_ms();
        if now > cache.created_at + Self::CACHE_TTL_MS {
            tracing::info!("OAuth login cache expired, will generate new session");
            Self::clear();
            return None;
        }

        tracing::info!("Loaded OAuth login cache from {:?}", path);
        Some(cache)
    }

    fn save(&self) -> Result<()> {
        let path = Self::cache_path();
        let content = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, content)?;
        tracing::info!("Saved OAuth login cache to {:?}", path);
        Ok(())
    }

    fn clear() {
        let path = Self::cache_path();
        if path.exists() {
            std::fs::remove_file(&path).ok();
            tracing::info!("Cleared OAuth login cache");
        }
    }
}

/// 用授权码交换 access token
///
/// 注意：token 请求使用 JSON 格式，并包含 state 参数
///
/// # 参数
///
/// * `code` - 授权码
/// * `verifier` - PKCE verifier
/// * `state` - OAuth state
/// * `redirect_uri` - 重定向 URI
pub async fn exchange_code(
    code: &str,
    verifier: &str,
    state: &str,
    redirect_uri: &str,
) -> Result<OAuthConfig> {
    let body = json!({
        "grant_type": "authorization_code",
        "code": code,
        "redirect_uri": redirect_uri,
        "client_id": CLAUDE_CODE_OAUTH_CLIENT_ID,
        "code_verifier": verifier,
        "state": state,
    });

    let response = token_request(&body).await?;
    parse_token_response(&response)
}

/// 刷新 access token
///
/// # 参数
///
/// * `refresh_token` - Refresh token
pub async fn refresh_token(refresh_token: &str) -> Result<OAuthConfig> {
    tracing::info!("Refreshing OAuth access token");

    let body = json!({
        "grant_type": "refresh_token",
        "refresh_token": refresh_token,
        "client_id": CLAUDE_CODE_OAUTH_CLIENT_ID,
        "scope": CLAUDE_CODE_OAUTH_SCOPES.join(" "),
    });

    let response = token_request(&body).await?;
    parse_token_response(&response)
}

/// 发送 token 请求（使用 JSON 格式）
async fn token_request(body: &serde_json::Value) -> Result<serde_json::Value> {
    let response = crate::utils::get_shared_client()
        .post(CLAUDE_CODE_OAUTH_TOKEN_URL)
        .header("Content-Type", "application/json")
        .json(body)
        .send()
        .await
        .context("OAuth request failed")?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        bail!("OAuth API error (HTTP {}): {}", status.as_u16(), body);
    }

    response
        .json()
        .await
        .context("Failed to parse OAuth response")
}

fn parse_token_response(json: &serde_json::Value) -> Result<OAuthConfig> {
    let access_token = json["access_token"]
        .as_str()
        .context("Missing access_token")?
        .to_string();

    let refresh_token = json["refresh_token"]
        .as_str()
        .context("Missing refresh_token")?
        .to_string();

    let expires_in = json["expires_in"].as_u64().context("Missing expires_in")?;
    let expires_at = unix_timestamp_ms() + (expires_in * 1000);

    let scopes = json["scope"]
        .as_str()
        .unwrap_or("")
        .split_whitespace()
        .map(String::from)
        .collect();

    Ok(OAuthConfig {
        access_token,
        refresh_token,
        expires_at,
        scopes,
    })
}

/// 从标准输入读取授权码
fn read_authorization_code() -> Result<String> {
    print!("Enter authorization code: ");
    io::stdout().flush()?;

    let mut code = String::new();
    io::stdin().read_line(&mut code)?;

    // 处理可能包含 state 的授权码格式（如 `code#state`）
    let code = code
        .trim()
        .split('#')
        .next()
        .unwrap_or("")
        .trim()
        .to_string();

    if code.is_empty() {
        bail!("Authorization code cannot be empty");
    }

    Ok(code)
}

/// 构建授权 URL
fn build_authorize_url(challenge: &str, state: &str) -> String {
    let scopes = CLAUDE_CODE_OAUTH_SCOPES.join(" ");
    format!(
        "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&state={}&code_challenge={}&code_challenge_method=S256",
        CLAUDE_CODE_OAUTH_AUTHORIZE_URL,
        CLAUDE_CODE_OAUTH_CLIENT_ID,
        urlencoding::encode(CLAUDE_CODE_OAUTH_REDIRECT_URI),
        urlencoding::encode(&scopes),
        urlencoding::encode(state),
        urlencoding::encode(challenge),
    )
}

/// 执行完整的 OAuth 登录流程
///
/// 打开浏览器进行授权，获取授权码，然后交换 token
/// 支持无限次重试，直到成功或用户中断
///
/// 注意：登录会话会被缓存，下次登录时如果缓存有效会复用相同的 URL
pub async fn perform_oauth_login() -> Result<OAuthConfig> {
    tracing::info!("Starting OAuth login flow");

    // 尝试加载缓存的登录会话
    let (verifier, state, authorize_url) = if let Some(cache) = OAuthLoginCache::load() {
        println!("Using cached OAuth session (delete cache to start fresh)");
        println!("Cache file: {:?}\n", OAuthLoginCache::cache_path());
        (cache.verifier, cache.state, cache.authorize_url)
    } else {
        // 生成新的 PKCE 和 state
        let pkce = PkceChallenge::generate();
        let state = generate_random_base64url();
        let authorize_url = build_authorize_url(&pkce.challenge, &state);

        // 缓存登录会话
        let cache = OAuthLoginCache {
            verifier: pkce.verifier.clone(),
            challenge: pkce.challenge,
            state: state.clone(),
            authorize_url: authorize_url.clone(),
            created_at: unix_timestamp_ms(),
        };
        if let Err(e) = cache.save() {
            tracing::warn!("Failed to save OAuth login cache: {}", e);
        }

        (pkce.verifier, state, authorize_url)
    };

    println!("Open the following URL in your browser to authorize:");
    println!("{}\n", authorize_url);

    if let Err(e) = open::that(&authorize_url) {
        tracing::warn!("Failed to open browser: {}", e);
    }

    loop {
        let code = match read_authorization_code() {
            Ok(code) => code,
            Err(e) => {
                eprintln!("Error: {}. Please try again.\n", e);
                continue;
            }
        };

        tracing::info!("Received authorization code");

        match exchange_code(&code, &verifier, &state, CLAUDE_CODE_OAUTH_REDIRECT_URI).await {
            Ok(config) => {
                // 登录成功，清除缓存
                OAuthLoginCache::clear();
                return Ok(config);
            }
            Err(e) => {
                eprintln!("Error: {}. Please try again.\n", e);
            }
        }
    }
}
