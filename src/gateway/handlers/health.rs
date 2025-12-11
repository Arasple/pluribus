//! 健康检查和版本信息处理器

use axum::{extract::State, Json};
use serde::Serialize;
use serde_json::json;

use crate::gateway::state::AppState;
use crate::providers::claude_code::get_claude_code_version;
use crate::providers::{ProviderType, RateLimitInfo};

/// Provider 状态信息
#[derive(Serialize)]
struct ProviderStatus {
    name: String,
    r#type: ProviderType,
    #[serde(skip_serializing_if = "Option::is_none")]
    rate_limit: Option<RateLimitInfo>,
}

/// 健康检查响应
#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    version: &'static str,
    providers: Vec<ProviderStatus>,
}

/// GET /health
pub async fn handle_health(State(state): State<AppState>) -> Json<serde_json::Value> {
    let providers: Vec<ProviderStatus> = state
        .providers()
        .iter()
        .map(|p| ProviderStatus {
            name: p.name().to_string(),
            r#type: p.provider_type(),
            rate_limit: p.rate_limit_info(),
        })
        .collect();

    Json(json!(HealthResponse {
        status: "ok",
        version: get_claude_code_version(),
        providers,
    }))
}
