//! Streaming synthesis. Filled in by Task 13.

use axum::response::{IntoResponse, Response};
use chorus_core::config::Profile;
use chorus_core::schema::ChatCompletionRequest;

use crate::state::AppState;

#[allow(clippy::unused_async)]
pub async fn stream_fusion(
    _state: AppState,
    _profile: Profile,
    _req: ChatCompletionRequest,
) -> Response {
    // Replaced in Task 13.
    axum::http::StatusCode::NOT_IMPLEMENTED.into_response()
}
