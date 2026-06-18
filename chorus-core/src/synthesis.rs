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
#[must_use]
pub fn synthesis_request(
    cfg: &AggregatorConfig,
    query: &str,
    responses: &[String],
    analysis: &str,
    stream: bool,
) -> ChatCompletionRequest {
    let references = format_references(responses, cfg.normalize_length, cfg.max_reference_chars);
    ChatCompletionRequest {
        model: cfg.synthesizer.clone(),
        messages: synthesis_messages(query, &references, analysis, cfg.single_source_cap),
        stream,
        temperature: Some(0.3),
    }
}

/// Call the synthesizer model and return the final grounded answer.
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
) -> Result<ChatCompletionResponse, Error> {
    let req = synthesis_request(cfg, query, responses, analysis, false);
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
        let req = synthesis_request(&agg(), "q", &["a".into()], "analysis", true);
        assert_eq!(req.model, "b/syn");
        assert!(req.stream);
        assert_eq!(req.messages.len(), 2);
    }
}
