//! HTTP 请求处理器

pub mod health;
pub mod messages;

pub use health::handle_health;
pub use messages::handle_anthropic_messages;

use axum::{http::StatusCode, response::IntoResponse, Json};
use serde::Serialize;

#[derive(Serialize)]
struct ErrorResponse {
    #[serde(rename = "type")]
    error_type: &'static str,
    message: String,
}

fn error_response(err: anyhow::Error) -> axum::response::Response {
    let error = ErrorResponse {
        error_type: "error",
        message: format!("{:#}", err),
    };
    (StatusCode::INTERNAL_SERVER_ERROR, Json(error)).into_response()
}
