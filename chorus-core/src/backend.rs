//! The backend abstraction: one OpenAI-compatible upstream.

use std::time::Duration;

use async_trait::async_trait;

use crate::error::Error;
use crate::schema::{ChatCompletionRequest, ChatCompletionResponse};

/// Anything chorus can send a chat-completion request to.
#[async_trait]
pub trait ChatBackend: Send + Sync {
    /// Send a chat-completion request and return the response.
    async fn complete(&self, req: &ChatCompletionRequest) -> Result<ChatCompletionResponse, Error>;
}

/// An OpenAI-compatible HTTP backend.
pub struct OpenAiBackend {
    client: reqwest::Client,
    base_url: String,
    api_key: String,
}

impl OpenAiBackend {
    /// `base_url` is the full prefix including any version segment, for example
    /// `http://localhost:8000/v1`. The request is sent to `{base_url}/chat/completions`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Backend`] if the underlying HTTP client cannot be built.
    pub fn new(
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        timeout: Duration,
    ) -> Result<Self, Error> {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| Error::Backend(e.to_string()))?;
        Ok(Self {
            client,
            base_url: base_url.into(),
            api_key: api_key.into(),
        })
    }
}

#[async_trait]
impl ChatBackend for OpenAiBackend {
    async fn complete(&self, req: &ChatCompletionRequest) -> Result<ChatCompletionResponse, Error> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let resp = self
            .client
            .post(url)
            .bearer_auth(&self.api_key)
            .json(req)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    Error::Timeout
                } else {
                    Error::Backend(e.to_string())
                }
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::Backend(format!("upstream {status}: {body}")));
        }

        resp.json::<ChatCompletionResponse>()
            .await
            .map_err(|e| Error::Backend(format!("decode: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::ChatMessage;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn sample_response_body() -> serde_json::Value {
        serde_json::json!({
            "id": "x", "object": "chat.completion", "created": 1, "model": "m",
            "choices": [{"index": 0, "message": {"role": "assistant", "content": "ok"},
                "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
        })
    }

    fn req() -> ChatCompletionRequest {
        ChatCompletionRequest {
            model: "m".into(),
            messages: vec![ChatMessage::user("hi")],
            stream: false,
            temperature: None,
        }
    }

    #[tokio::test]
    async fn complete_returns_parsed_response() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(sample_response_body()))
            .mount(&server)
            .await;

        let backend = OpenAiBackend::new(
            format!("{}/v1", server.uri()),
            "secret",
            Duration::from_secs(5),
        )
        .unwrap();

        let resp = backend.complete(&req()).await.unwrap();
        assert_eq!(resp.first_content(), "ok");
    }

    #[tokio::test]
    async fn upstream_error_becomes_backend_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(429).set_body_string("rate limited"))
            .mount(&server)
            .await;

        let backend = OpenAiBackend::new(
            format!("{}/v1", server.uri()),
            "secret",
            Duration::from_secs(5),
        )
        .unwrap();

        let err = backend.complete(&req()).await.unwrap_err();
        assert!(matches!(err, Error::Backend(_)));
    }
}
