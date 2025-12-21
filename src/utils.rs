use reqwest::Client;
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// 是否禁用 TLS 验证（用于调试 mitmproxy 等场景）
pub fn should_disable_tls_verify() -> bool {
    std::env::var("PLURIBUS_DISABLE_TLS_VERIFY")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// 获取共享的 HTTP 客户端（用于一般请求，如 OAuth、版本查询等）
static SHARED_CLIENT: OnceLock<Client> = OnceLock::new();

pub fn get_shared_client() -> &'static Client {
    SHARED_CLIENT.get_or_init(|| {
        let mut builder = Client::builder().timeout(Duration::from_secs(30));

        if should_disable_tls_verify() {
            tracing::warn!("TLS certificate verification is DISABLED - for debugging only!");
            builder = builder.danger_accept_invalid_certs(true);
        }

        builder.build().expect("Failed to create HTTP client")
    })
}

/// 获取当前 Unix 时间戳（毫秒）
///
/// # 返回值
///
/// 返回自 1970-01-01 00:00:00 UTC 以来的毫秒数
///
/// # 注意
///
/// 如果系统时钟早于 UNIX_EPOCH（极端情况），返回 0
#[inline]
pub fn unix_timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 从请求体中提取 model 字段
///
/// # 参数
///
/// * `body` - JSON 格式的请求体
///
/// # 返回值
///
/// 返回 model 字段的值，如果不存在则返回 "unknown"
#[inline]
pub fn extract_model(body: &serde_json::Value) -> String {
    body.get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string()
}
