//! The synthesizer: the final grounded answer under the hardened prompt.

use std::sync::Arc;

use crate::backend::ChatBackend;
use crate::config::AggregatorConfig;
use crate::error::Error;
use crate::prompts::{format_references, synthesis_messages};
use crate::schema::{ChatCompletionRequest, ChatCompletionResponse};

/// Build the synthesis request.
///
/// This is a separate public function so the streaming path in the server can reuse the
/// same request construction without going through [`run_synthesis`].
///
/// `max_tokens` is forwarded directly from the caller's inbound request so the final
/// answer is bounded the same way a single-model call would be.
#[must_use]
pub fn synthesis_request(
    cfg: &AggregatorConfig,
    query: &str,
    responses: &[String],
    analysis: &str,
    stream: bool,
    max_tokens: Option<u32>,
) -> ChatCompletionRequest {
    let references = format_references(responses, cfg.normalize_length, cfg.max_reference_chars);
    ChatCompletionRequest {
        model: cfg.synthesizer.clone(),
        messages: synthesis_messages(query, &references, analysis, cfg.single_source_cap),
        stream,
        temperature: Some(0.3),
        max_tokens,
    }
}

/// Call the synthesizer model and return the final grounded answer.
///
/// `max_tokens` is forwarded from the caller's inbound request so the final answer
/// is bounded the same way a single-model call would be.
///
/// The synthesizer is a single call with no quorum to fall back on, so unlike a
/// panel member a transient failure (a slow or flaky upstream, a truncated or
/// undecodable body) gets one bounded retry before the error is surfaced. The
/// pipeline degrades to a panel answer if the retry also fails (issue #32).
///
/// # Errors
///
/// Returns [`Error::Synthesis`] wrapping the second backend error if both the
/// initial call and the single retry fail.
pub async fn run_synthesis(
    backend: Arc<dyn ChatBackend>,
    cfg: &AggregatorConfig,
    query: &str,
    responses: &[String],
    analysis: &str,
    max_tokens: Option<u32>,
) -> Result<ChatCompletionResponse, Error> {
    let req = synthesis_request(cfg, query, responses, analysis, false, max_tokens);
    match backend.complete(&req).await {
        Ok(resp) => Ok(resp),
        Err(first) => {
            tracing::warn!(error = %first, "synthesizer call failed; retrying once");
            backend
                .complete(&req)
                .await
                .map_err(|e| Error::Synthesis(e.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AggregatorConfig;
    use crate::schema::{ChatMessage, Choice};
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn synth_resp(content: &str) -> ChatCompletionResponse {
        ChatCompletionResponse {
            id: "x".into(),
            object: "chat.completion".into(),
            created: 1,
            model: "b/syn".into(),
            choices: vec![Choice {
                index: 0,
                message: ChatMessage {
                    role: "assistant".into(),
                    content: content.into(),
                },
                finish_reason: Some("stop".into()),
            }],
            usage: None,
        }
    }

    /// Fails its first call with a decode-style transient error, then succeeds.
    struct FlakyOnce {
        calls: AtomicUsize,
    }

    #[async_trait]
    impl ChatBackend for FlakyOnce {
        async fn complete(
            &self,
            _req: &ChatCompletionRequest,
        ) -> Result<ChatCompletionResponse, Error> {
            if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                Err(Error::Backend(
                    "decode: error decoding response body".into(),
                ))
            } else {
                Ok(synth_resp("SYNTH"))
            }
        }
    }

    struct AlwaysFails;

    #[async_trait]
    impl ChatBackend for AlwaysFails {
        async fn complete(
            &self,
            _req: &ChatCompletionRequest,
        ) -> Result<ChatCompletionResponse, Error> {
            Err(Error::Backend(
                "decode: error decoding response body".into(),
            ))
        }
    }

    #[tokio::test]
    async fn run_synthesis_retries_once_on_transient_error() {
        let backend = Arc::new(FlakyOnce {
            calls: AtomicUsize::new(0),
        });
        let out = run_synthesis(
            Arc::clone(&backend) as Arc<dyn ChatBackend>,
            &agg(),
            "q",
            &["a".into()],
            "analysis",
            None,
        )
        .await
        .expect("retry should recover the transient failure");
        assert_eq!(out.first_content(), "SYNTH");
        // One failed call plus one successful retry.
        assert_eq!(backend.calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn run_synthesis_surfaces_synthesis_error_after_retry_exhausted() {
        let out = run_synthesis(
            Arc::new(AlwaysFails),
            &agg(),
            "q",
            &["a".into()],
            "analysis",
            None,
        )
        .await;
        assert!(matches!(out, Err(Error::Synthesis(_))));
    }

    fn agg() -> AggregatorConfig {
        AggregatorConfig {
            judge: "b/judge".into(),
            synthesizer: "b/syn".into(),
            anonymize_sources: true,
            normalize_length: true,
            single_source_cap: true,
            layers: 1,
            max_reference_chars: 8_000,
        }
    }

    #[test]
    fn synthesis_request_targets_synthesizer_and_sets_stream() {
        let req = synthesis_request(&agg(), "q", &["a".into()], "analysis", true, None);
        assert_eq!(req.model, "b/syn");
        assert!(req.stream);
        assert_eq!(req.messages.len(), 2);
        assert_eq!(req.max_tokens, None);
    }

    #[test]
    fn synthesis_request_forwards_max_tokens() {
        let req = synthesis_request(&agg(), "q", &["a".into()], "analysis", false, Some(512));
        assert_eq!(req.max_tokens, Some(512));
    }
}
