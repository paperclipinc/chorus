//! Build the axum application.

use axum::Router;
use axum::routing::{get, post};
use tower_http::trace::TraceLayer;

use crate::handlers;
use crate::state::AppState;

/// Construct the axum [`Router`] with all routes and middleware attached.
pub fn build(state: AppState) -> Router {
    Router::new()
        .route("/v1/chat/completions", post(handlers::chat_completions))
        .route("/v1/models", get(handlers::models))
        .route("/healthz", get(handlers::healthz))
        .route("/metrics", get(handlers::metrics))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
