//! Gateway 中间件

use axum::{
    extract::Request,
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use std::sync::atomic::{AtomicU64, Ordering};
use subtle::ConstantTimeEq;
use tracing::Instrument;

/// 全局请求计数器，用于生成 request_id
static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(1);

/// 认证错误响应
#[derive(Serialize)]
struct AuthError {
    #[serde(rename = "type")]
    error_type: &'static str,
    message: &'static str,
}

/// Secret 认证中间件
pub async fn auth_middleware(secret: String, request: Request, next: Next) -> Response {
    let provided = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .or_else(|| {
            request
                .headers()
                .get("x-api-key")
                .and_then(|v| v.to_str().ok())
        });

    let is_valid = provided
        .map(|p| p.as_bytes().ct_eq(secret.as_bytes()).into())
        .unwrap_or(false);

    if is_valid {
        return next.run(request).await;
    }

    let error = AuthError {
        error_type: "authentication_error",
        message: "Invalid or missing secret",
    };
    (StatusCode::UNAUTHORIZED, Json(error)).into_response()
}

/// 请求日志中间件
pub async fn request_logger(request: Request, next: Next) -> Response {
    let request_id = REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    let method = request.method().clone();
    let path = request.uri().path().to_string();

    let span = tracing::info_span!(
        "req",
        id = request_id,
        %method,
        %path,
    );

    async move {
        let start = std::time::Instant::now();
        let response = next.run(request).await;
        let latency_ms = start.elapsed().as_millis() as u64;
        let status = response.status().as_u16();

        tracing::info!(status, latency_ms, "done");

        response
    }
    .instrument(span)
    .await
}
