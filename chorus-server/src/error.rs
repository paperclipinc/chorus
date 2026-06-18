//! Map a core Error to an OpenAI-shaped error response.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use chorus_core::Error;
use serde_json::json;

pub struct ApiError(pub Error);

impl From<Error> for ApiError {
    fn from(e: Error) -> Self {
        ApiError(e)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, kind) = match &self.0 {
            Error::UnknownProfile(_) | Error::InvalidModel(_) => {
                (StatusCode::NOT_FOUND, "invalid_request_error")
            }
            Error::Config(_) => (StatusCode::BAD_REQUEST, "invalid_request_error"),
            Error::Quorum { .. } | Error::Backend(_) | Error::Synthesis(_) => {
                (StatusCode::BAD_GATEWAY, "upstream_error")
            }
            Error::Timeout => (StatusCode::GATEWAY_TIMEOUT, "timeout"),
        };
        let body = json!({
            "error": { "message": self.0.to_string(), "type": kind }
        });
        (status, Json(body)).into_response()
    }
}
