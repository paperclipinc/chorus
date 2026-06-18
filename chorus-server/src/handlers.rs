//! HTTP handlers for the `OpenAI` surface.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chorus_core::Error;
use chorus_core::schema::ChatCompletionRequest;
use serde_json::json;

use crate::error::ApiError;
use crate::state::AppState;

#[allow(clippy::unused_async)]
pub async fn healthz() -> StatusCode {
    StatusCode::OK
}

pub async fn metrics(State(state): State<AppState>) -> String {
    state.metrics.render()
}

pub async fn models(State(state): State<AppState>) -> Json<serde_json::Value> {
    let data: Vec<_> = state
        .config
        .profiles
        .iter()
        .map(|p| json!({ "id": format!("fusion/{}", p.name), "object": "model" }))
        .collect();
    Json(json!({ "object": "list", "data": data }))
}

/// Resolve the profile name from a `fusion/<name>` model alias.
fn profile_name(model: &str) -> Result<&str, Error> {
    model
        .strip_prefix("fusion/")
        .ok_or_else(|| Error::InvalidModel(model.to_string()))
}

/// # Errors
///
/// Returns an [`ApiError`] if the model alias is invalid, the profile is not found,
/// the concurrency limiter is closed, or the pipeline returns an error.
pub async fn chat_completions(
    State(state): State<AppState>,
    Json(req): Json<ChatCompletionRequest>,
) -> Result<Response, ApiError> {
    let name = profile_name(&req.model)?;
    let profile = state
        .config
        .profile(name)
        .ok_or_else(|| Error::UnknownProfile(name.to_string()))?
        .clone();

    // Bound the fan-out amplification: one request holds one permit for its lifetime.
    let _permit = state
        .limiter
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| Error::Backend("limiter closed".into()))?;

    metrics::counter!("chorus_requests_total", "profile" => name.to_string()).increment(1);

    if req.stream {
        return Ok(crate::sse::stream_fusion(state.clone(), profile, req).await);
    }

    let resp = state.pipeline.run(&profile, &req).await?;
    Ok((StatusCode::OK, Json(resp)).into_response())
}
