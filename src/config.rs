//! 应用配置模块
//!
//! 负责从环境变量加载应用配置，包括：
//! - 服务器监听地址和端口
//! - 认证密钥
//! - Provider 配置文件存储路径

use anyhow::{Context, Result};
use std::path::PathBuf;

/// 应用配置
///
/// 包含服务器运行所需的所有配置项
#[derive(Debug, Clone)]
pub struct Config {
    /// 服务器监听地址（如 "0.0.0.0" 或 "127.0.0.1"）
    pub host: String,
    /// 服务器监听端口
    pub port: u16,
    /// API 访问密钥（用于 Bearer token 认证）
    pub secret: String,
    /// Provider 配置文件存储目录
    pub providers_dir: PathBuf,
}

impl Config {
    /// 从环境变量加载配置
    ///
    /// # 环境变量
    ///
    /// - `PLURIBUS_HOST`: 服务器监听地址（默认: "0.0.0.0"）
    /// - `PLURIBUS_PORT`: 服务器监听端口（默认: 8080）
    /// - `PLURIBUS_SECRET`: API 访问密钥（**必需**）
    ///
    /// # 错误
    ///
    /// - 如果 `PLURIBUS_SECRET` 未设置
    /// - 如果 `PLURIBUS_PORT` 不是有效的端口号
    pub fn from_env() -> Result<Self> {
        let host = std::env::var("PLURIBUS_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());

        let port = std::env::var("PLURIBUS_PORT")
            .unwrap_or_else(|_| "8080".to_string())
            .parse()
            .context("PLURIBUS_PORT must be a valid port number")?;

        let secret = std::env::var("PLURIBUS_SECRET")
            .context("PLURIBUS_SECRET environment variable is required")?;

        let providers_dir = PathBuf::from("./providers");

        Ok(Self {
            host,
            port,
            secret,
            providers_dir,
        })
    }

    /// 获取 provider 配置目录路径
    pub fn providers_dir(&self) -> &std::path::Path {
        &self.providers_dir
    }

    /// 确保必要的目录存在
    ///
    /// 创建 providers 配置目录（如果不存在）
    pub fn ensure_dirs(&self) -> Result<()> {
        std::fs::create_dir_all(&self.providers_dir)
            .context("Failed to create providers directory")?;
        Ok(())
    }
}
