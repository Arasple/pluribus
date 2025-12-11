//! Serve 命令 - 启动 API 服务器
//!
//! 此模块实现 `serve` 命令，启动 HTTP API 服务器以代理 AI Provider 请求。

use anyhow::Result;

use crate::config::Config;
use crate::gateway;

/// 执行服务器启动命令
///
/// # 参数
///
/// * `config` - 应用配置，包含监听地址、端口等信息
///
/// # 功能
///
/// - 加载所有已配置的 Provider
/// - 初始化 HTTP 路由和中间件
/// - 启动服务器并等待关闭信号
/// - 支持优雅关闭（Ctrl+C 或 SIGTERM）
///
/// # 返回
///
/// 成功时返回 Ok(())，失败时返回错误信息
pub async fn serve_command(config: Config) -> Result<()> {
    gateway::serve(config).await
}
