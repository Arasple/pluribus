//! Login 命令 - OAuth 登录流程
//!
//! 此模块实现 `login` 命令，用于通过 OAuth 流程登录到各种 AI Provider。
//! 当前支持 Claude Code 的 OAuth 认证。

use anyhow::{Context, Result};

use crate::config::Config;
use crate::providers::claude_code;
use crate::providers::{AuthConfig, ProviderConfig, ProviderType};

/// 执行登录命令
///
/// # 参数
///
/// * `app_config` - 应用配置
/// * `provider_type` - Provider 类型（如 ClaudeCode）
/// * `name` - 可选的 Provider 实例名称（如果未提供，使用默认名称）
///
/// # 工作流程
///
/// 1. 打开浏览器进行 OAuth 授权
/// 2. 用户在浏览器中完成授权并获取授权码
/// 3. 用授权码交换 access token 和 refresh token
/// 4. 将认证信息保存到配置文件
///
/// # 返回
///
/// 成功时返回 Ok(())，失败时返回错误信息
pub async fn login_command(
    app_config: Config,
    provider_type: ProviderType,
    name: Option<String>,
) -> Result<()> {
    // 如果用户未提供名称，使用 Provider 类型的默认名称
    let provider_name = name.unwrap_or_else(|| match provider_type {
        ProviderType::ClaudeCode => "claude-code".to_string(),
        ProviderType::Anthropic => "anthropic".to_string(),
        ProviderType::OpenAI => "openai".to_string(),
        ProviderType::Codex => "codex".to_string(),
    });

    match provider_type {
        ProviderType::ClaudeCode => {
            println!("Starting Claude Code OAuth login...\n");

            // 执行 OAuth 登录流程
            let oauth = claude_code::perform_oauth_login()
                .await
                .context("OAuth login failed")?;

            let providers_dir = app_config.providers_dir();

            // 创建 Provider 配置
            let config = ProviderConfig {
                name: provider_name.clone(),
                provider_type: ProviderType::ClaudeCode,
                auth: AuthConfig::OAuth(oauth.clone()),
            };

            // 保存配置到文件
            crate::providers::save(providers_dir, &provider_name, &config)
                .await
                .context("Failed to save provider config")?;

            // 显示成功信息
            println!("\nLogin successful!");
            println!("Provider: {}", provider_name);
            println!(
                "Config file: {}/{}.toml",
                providers_dir.display(),
                provider_name
            );
            if !oauth.scopes.is_empty() {
                println!("Scopes: {}", oauth.scopes.join(", "));
            }
            Ok(())
        }
        // 其他 Provider 类型暂不支持
        _ => anyhow::bail!("Provider {:?} not yet supported", provider_type),
    }
}
