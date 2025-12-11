//! Gateway 应用层
//!
//! HTTP 服务器和请求处理

mod handlers;
mod middleware;
mod state;

pub use state::AppState;

use anyhow::Result;
use axum::{
    http::StatusCode,
    middleware as axum_middleware,
    routing::{get, post},
    Router,
};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::signal;
use tower::ServiceBuilder;
use tower_http::{timeout::TimeoutLayer, trace::TraceLayer};

use crate::config::Config;
use crate::providers::{self, claude_code};

const DEFAULT_REQUEST_TIMEOUT_SECS: u64 = 300;

pub async fn serve(config: Config) -> Result<()> {
    claude_code::init_version().await?;
    config.ensure_dirs()?;

    let providers = providers::load_providers(config.providers_dir()).await?;
    let state = AppState::new(providers);
    let app = build_router(state, &config);
    let addr: SocketAddr = format!("{}:{}", config.host, config.port).parse()?;
    tracing::info!("Starting server on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    tracing::info!("Server shutdown complete");
    Ok(())
}

fn build_router(state: AppState, config: &Config) -> Router {
    let secret = config.secret.clone();

    let public_routes = Router::new().route("/health", get(handlers::handle_health));
    let api_routes = Router::new()
        .route(
            "/anthropic/v1/messages",
            post(handlers::handle_anthropic_messages),
        )
        .route_layer(axum_middleware::from_fn(move |req, next| {
            let secret = secret.clone();
            middleware::auth_middleware(secret, req, next)
        }));

    Router::new()
        .merge(api_routes)
        .merge(public_routes)
        .layer(
            ServiceBuilder::new()
                .layer(axum_middleware::from_fn(middleware::request_logger))
                .layer(TraceLayer::new_for_http())
                .layer(TimeoutLayer::with_status_code(
                    StatusCode::REQUEST_TIMEOUT,
                    Duration::from_secs(DEFAULT_REQUEST_TIMEOUT_SECS),
                )),
        )
        .with_state(state)
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(unix)]
    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    #[cfg(not(unix))]
    tokio::select! {
        _ = ctrl_c => {},
    }

    tracing::info!("Shutdown signal received, starting graceful shutdown...");
}
