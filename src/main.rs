//! Pluribus - Claude Code API 中继服务
//!
//! 一个轻量级的 API 网关，用于代理和管理多个 Claude Code Provider。
//!
//! # 功能特性
//!
//! - 支持 OAuth 认证的 Claude Code 订阅账号
//! - 自动 token 刷新机制
//! - Round-robin 负载均衡
//! - Rate limit 监控和上报
//! - 流式和非流式请求支持
//!
//! # 命令行接口
//!
//! - `serve`: 启动 API 服务器
//! - `login`: 通过 OAuth 登录添加 Provider
//! - `test`: 向本地服务器发送测试请求

mod commands;
mod config;
mod gateway;
mod providers;
mod utils;

use anyhow::Result;
use clap::{Parser, Subcommand};
use config::Config;
use providers::ProviderType;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

/// Pluribus CLI
#[derive(Parser)]
#[command(name = "pluribus")]
#[command(about = "Claude Code API Relay Service", long_about = None)]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

/// 可用的命令
#[derive(Subcommand)]
enum Commands {
    /// 启动 API 中继服务器
    Serve,
    /// 通过 OAuth 登录到 Provider
    Login {
        /// Provider 类型
        #[arg(value_enum)]
        provider: ProviderType,
        /// 为此 Provider 实例指定自定义名称
        #[arg(short, long)]
        name: Option<String>,
    },
    /// 向本地服务器发送测试请求
    Test,
}

#[tokio::main]
async fn main() -> Result<()> {
    // 加载 .env 文件（如果存在）
    if let Ok(dotenv_path) = std::env::var("PLURIBUS_ENV_FILE") {
        dotenvy::from_path(&dotenv_path).ok();
    } else {
        dotenvy::dotenv().ok();
    }

    // 初始化日志系统
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "pluribus=info".into()),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(false)
                .with_thread_ids(false)
                .with_thread_names(false),
        )
        .init();

    // 解析命令行参数和配置
    let cli = Cli::parse();
    let config = Config::from_env()?;

    // 执行相应的命令
    match cli.command {
        Commands::Serve => commands::serve_command(config).await,
        Commands::Login { provider, name } => commands::login_command(config, provider, name).await,
        Commands::Test => commands::test_command(config).await,
    }
}
