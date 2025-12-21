//! Messages API 处理器

use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, Response},
    Json,
};
use serde_json::Value;

use crate::gateway::{handlers::error_response, state::AppState};
use crate::providers::parse_anthropic_usage;
use crate::utils::extract_model;

/// 需要透传的 header 名称
const PASSTHROUGH_HEADERS: &[&str] = &["anthropic-beta"];

/// POST /anthropic/v1/messages 处理器
pub async fn handle_anthropic_messages(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(mut body): Json<Value>,
) -> axum::response::Response {
    // 将需要透传的 headers 注入到 body 的 _passthrough_headers 字段
    if let Some(obj) = body.as_object_mut() {
        let mut passthrough = serde_json::Map::new();
        for &name in PASSTHROUGH_HEADERS {
            if let Some(value) = headers.get(name).and_then(|v| v.to_str().ok()) {
                passthrough.insert(name.to_string(), Value::String(value.to_string()));
            }
        }
        if !passthrough.is_empty() {
            obj.insert(
                "_passthrough_headers".to_string(),
                Value::Object(passthrough),
            );
        }
    }

    let result: anyhow::Result<Response<Body>> = async {
        // 轮询选择一个 provider
        let provider = state
            .get_next_provider(|p| p.provider_type().is_anthropic())
            .ok_or_else(|| anyhow::anyhow!("No provider available. Run 'pluribus login' first."))?;

        let provider_name = provider.name();
        let model = extract_model(&body);

        // 检查是否为流式请求
        let is_streaming = body
            .get("stream")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        tracing::info!(
            provider = provider_name,
            model,
            streaming = is_streaming,
            "request"
        );

        if is_streaming {
            // 流式请求
            let streaming_response = provider.send_streaming(body).await?;

            let response = Response::builder()
                .status(streaming_response.status)
                .header("content-type", "text/event-stream")
                .header("cache-control", "no-cache")
                .header("connection", "keep-alive")
                .body(Body::from_stream(streaming_response.stream))
                .map_err(|e| anyhow::anyhow!("Failed to build streaming response: {}", e))?;

            Ok(response)
        } else {
            // 非流式请求
            let response_body = provider.send_message(body).await?;
            let usage = parse_anthropic_usage(&response_body).unwrap_or_default();

            tracing::info!(
                provider = provider_name,
                model,
                input_tokens = usage.input_tokens,
                output_tokens = usage.output_tokens,
                cache_read = usage.cache_read_tokens,
                cache_write = usage.cache_creation_tokens,
                "response"
            );

            let response = Response::builder()
                .status(200)
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_string(&response_body)?))
                .map_err(|e| anyhow::anyhow!("Failed to build response: {}", e))?;

            Ok(response)
        }
    }
    .await;

    match result {
        Ok(response) => response,
        Err(err) => error_response(err),
    }
}
