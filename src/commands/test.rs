//! Test 命令 - 发送测试请求到本地服务器
//!
//! 此模块实现 `test` 命令，用于向本地运行的 Pluribus 服务器发送测试请求，
//! 验证服务是否正常工作。

use anyhow::{Context, Result};

use crate::config::Config;

/// 执行测试命令
///
/// # 参数
///
/// * `config` - 应用配置，用于获取服务器地址和认证密钥
///
/// # 功能
///
/// - 向本地服务器的 `/anthropic/v1/messages` 端点发送一个简单的测试请求
/// - 使用配置的 secret 进行认证
/// - 显示响应状态和内容
///
/// # 测试请求内容
///
/// 使用 `claude-haiku-4-5` 模型发送一条简单的问候消息
///
/// # 返回
///
/// 成功时返回 Ok(())，失败时返回错误信息
pub async fn test_command(config: Config) -> Result<()> {
    println!("Sending test request to local server...");

    // 构造测试请求体
    let test_body = serde_json::json!({
        "model": "claude-haiku-4-5",
        "max_tokens": 100,
        "messages": [
            {
                "role": "user",
                "content": "哈喽，克劳德 👋。"
            }
        ]
    });

    let url = format!(
        "http://{}:{}/anthropic/v1/messages",
        config.host, config.port
    );

    println!("Request URL: {}", url);

    // 发送请求
    let response = reqwest::Client::new()
        .post(&url)
        .header("Authorization", format!("Bearer {}", config.secret))
        .json(&test_body)
        .send()
        .await
        .context("Request failed. Make sure the server is running.")?;

    let status = response.status();
    println!("Response status: {}", status);

    // 检查响应状态
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        anyhow::bail!("Request failed: {}", body);
    }

    // 显示响应内容
    let body = response
        .text()
        .await
        .context("Failed to read response body")?;

    println!("Response:");
    println!("{}", body);

    Ok(())
}
