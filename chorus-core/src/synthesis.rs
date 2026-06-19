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
/// # Errors
///
/// Returns [`Error::Synthesis`] wrapping any backend error.
pub async fn run_synthesis(
    backend: Arc<dyn ChatBackend>,
    cfg: &AggregatorConfig,
    query: &str,
    responses: &[String],
    analysis: &str,
    max_tokens: Option<u32>,
) -> Result<ChatCompletionResponse, Error> {
    let req = synthesis_request(cfg, query, responses, analysis, false, max_tokens);
    backend
        .complete(&req)
        .await
        .map_err(|e| Error::Synthesis(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AggregatorConfig;

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
