//! OAuth 客户端和登录流程

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::io::{self, Write};

use crate::providers::OAuthConfig;
use crate::utils::unix_timestamp_ms;

use super::constants::{
    PkceChallenge, CLAUDE_CODE_OAUTH_AUTHORIZE_URL, CLAUDE_CODE_OAUTH_CLIENT_ID,
    CLAUDE_CODE_OAUTH_REDIRECT_URI, CLAUDE_CODE_OAUTH_SCOPES, CLAUDE_CODE_OAUTH_TOKEN_URL,
};

/// 用授权码交换 access token
///
/// # 参数
///
/// * `code` - 授权码
/// * `verifier` - PKCE verifier
/// * `redirect_uri` - 重定向 URI
pub async fn exchange_code(code: &str, verifier: &str, redirect_uri: &str) -> Result<OAuthConfig> {
    let body = json!({
        "grant_type": "authorization_code",
        "code": code,
        "redirect_uri": redirect_uri,
        "client_id": CLAUDE_CODE_OAUTH_CLIENT_ID,
        "code_verifier": verifier,
    });

    let response = token_request(body).await?;
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
    });

    let response = token_request(body).await?;
    parse_token_response(&response)
}

async fn token_request(body: Value) -> Result<Value> {
    let response = crate::utils::get_shared_client()
        .post(CLAUDE_CODE_OAUTH_TOKEN_URL)
        .json(&body)
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

fn parse_token_response(json: &Value) -> Result<OAuthConfig> {
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

/// 执行完整的 OAuth 登录流程
///
/// 打开浏览器进行授权，获取授权码，然后交换 token
pub async fn perform_oauth_login() -> Result<OAuthConfig> {
    tracing::info!("Starting OAuth login flow");

    let pkce = PkceChallenge::generate();

    let scopes = CLAUDE_CODE_OAUTH_SCOPES.join(" ");
    let authorize_url = format!(
        "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&code_challenge={}&code_challenge_method=S256",
        CLAUDE_CODE_OAUTH_AUTHORIZE_URL,
        CLAUDE_CODE_OAUTH_CLIENT_ID,
        urlencoding::encode(CLAUDE_CODE_OAUTH_REDIRECT_URI),
        urlencoding::encode(&scopes),
        urlencoding::encode(&pkce.challenge),
    );

    println!("Open the following URL in your browser to authorize:");
    println!("{}\n", authorize_url);

    if let Err(e) = open::that(&authorize_url) {
        tracing::warn!("Failed to open browser: {}", e);
    }

    print!("Enter authorization code: ");
    io::stdout().flush()?;

    let mut code = String::new();
    io::stdin().read_line(&mut code)?;
    let code = code.trim();

    if code.is_empty() {
        bail!("Authorization code cannot be empty");
    }

    tracing::info!("Received authorization code");

    exchange_code(code, &pkce.verifier, CLAUDE_CODE_OAUTH_REDIRECT_URI).await
}
