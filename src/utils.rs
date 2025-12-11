use std::time::{SystemTime, UNIX_EPOCH};

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
