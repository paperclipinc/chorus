//! The router gate: decide whether to fuse or forward to a single model.

use std::sync::Arc;

use async_trait::async_trait;

use crate::backend::ChatBackend;
use crate::prompts::{difficulty_messages, parse_difficulty};
use crate::schema::ChatCompletionRequest;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteDecision {
    Fuse,
    Single,
}

#[async_trait]
pub trait Router: Send + Sync {
    async fn decide(&self, req: &ChatCompletionRequest) -> RouteDecision;
}

/// The M1 policy: always fuse. Removes the routing confound from the quality benchmark.
pub struct AlwaysFuse;

#[async_trait]
impl Router for AlwaysFuse {
    async fn decide(&self, _req: &ChatCompletionRequest) -> RouteDecision {
        RouteDecision::Fuse
    }
}

/// Routes by a cheap LLM-judge difficulty score: fuse when score >= threshold.
/// Fails OPEN to fusion on any backend error or unparseable score.
pub struct ClassifierRouter {
    backend: Arc<dyn ChatBackend>,
    model: String,
    threshold: f32,
}

impl ClassifierRouter {
    /// Create a new `ClassifierRouter`.
    ///
    /// `threshold` is the difficulty score above which the router fuses (range 0.0..=1.0).
    #[must_use]
    pub fn new(backend: Arc<dyn ChatBackend>, model: String, threshold: f32) -> Self {
        Self {
            backend,
            model,
            threshold,
        }
    }
}

#[async_trait]
impl Router for ClassifierRouter {
    async fn decide(&self, req: &ChatCompletionRequest) -> RouteDecision {
        let scoring = ChatCompletionRequest {
            model: self.model.clone(),
            messages: difficulty_messages(req.last_user_text()),
            stream: false,
            temperature: Some(0.0),
        };
        match self.backend.complete(&scoring).await {
            Ok(resp) => match parse_difficulty(resp.first_content()) {
                Some(score) if score < self.threshold => RouteDecision::Single,
                Some(_) => RouteDecision::Fuse,
                None => {
                    tracing::warn!("router score unparseable; failing open to fuse");
                    RouteDecision::Fuse
                }
            },
            Err(e) => {
                tracing::warn!(error = %e, "router scoring failed; failing open to fuse");
                RouteDecision::Fuse
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::ChatMessage;

    #[tokio::test]
    async fn always_fuse_fuses() {
        let r = AlwaysFuse;
        let req = ChatCompletionRequest {
            model: "fusion/research".into(),
            messages: vec![ChatMessage::user("hi")],
            stream: false,
            temperature: None,
        };
        assert_eq!(r.decide(&req).await, RouteDecision::Fuse);
    }
}

#[cfg(test)]
mod classifier_tests {
    use super::*;
    use crate::error::Error;
    use crate::schema::{ChatCompletionResponse, ChatMessage, Choice, Usage};

    struct ScoreBackend {
        reply: Result<&'static str, ()>,
    }

    #[async_trait]
    impl ChatBackend for ScoreBackend {
        async fn complete(
            &self,
            _req: &ChatCompletionRequest,
        ) -> Result<ChatCompletionResponse, Error> {
            match self.reply {
                Ok(text) => Ok(ChatCompletionResponse {
                    id: "x".into(),
                    object: "chat.completion".into(),
                    created: 1,
                    model: "cheap".into(),
                    choices: vec![Choice {
                        index: 0,
                        message: ChatMessage {
                            role: "assistant".into(),
                            content: text.into(),
                        },
                        finish_reason: Some("stop".into()),
                    }],
                    usage: Some(Usage::default()),
                }),
                Err(()) => Err(Error::Backend("down".into())),
            }
        }
    }

    fn req() -> ChatCompletionRequest {
        ChatCompletionRequest {
            model: "fusion/research".into(),
            messages: vec![ChatMessage::user("hi")],
            stream: false,
            temperature: None,
        }
    }

    #[tokio::test]
    async fn hard_query_fuses() {
        let b = Arc::new(ScoreBackend { reply: Ok("0.9") });
        let r = ClassifierRouter::new(b, "cheap".into(), 0.5);
        assert_eq!(r.decide(&req()).await, RouteDecision::Fuse);
    }

    #[tokio::test]
    async fn easy_query_goes_single() {
        let b = Arc::new(ScoreBackend { reply: Ok("0.1") });
        let r = ClassifierRouter::new(b, "cheap".into(), 0.5);
        assert_eq!(r.decide(&req()).await, RouteDecision::Single);
    }

    #[tokio::test]
    async fn backend_error_fails_open_to_fuse() {
        let b = Arc::new(ScoreBackend { reply: Err(()) });
        let r = ClassifierRouter::new(b, "cheap".into(), 0.5);
        assert_eq!(r.decide(&req()).await, RouteDecision::Fuse);
    }

    #[tokio::test]
    async fn unparseable_score_fails_open_to_fuse() {
        let b = Arc::new(ScoreBackend { reply: Ok("dunno") });
        let r = ClassifierRouter::new(b, "cheap".into(), 0.5);
        assert_eq!(r.decide(&req()).await, RouteDecision::Fuse);
    }
}
