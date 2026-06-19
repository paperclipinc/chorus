//! The judge: a structured analysis over anonymized panel answers.

use std::sync::Arc;

use crate::backend::ChatBackend;
use crate::config::AggregatorConfig;
use crate::error::Error;
use crate::prompts::{format_references, judge_messages};
use crate::schema::{ChatCompletionRequest, Usage};

/// The outcome of a judge call: a structured analysis text and token usage.
#[must_use]
pub struct JudgeOutcome {
    /// The structured analysis produced by the judge model.
    pub analysis: String,
    /// Token usage for the judge call, if the backend reported it.
    pub usage: Option<Usage>,
}

/// Call the judge model with anonymized panel responses and return a structured analysis.
///
/// # Errors
///
/// Propagates any [`Error`] returned by the backend.
pub async fn run_judge(
    backend: Arc<dyn ChatBackend>,
    cfg: &AggregatorConfig,
    query: &str,
    responses: &[String],
) -> Result<JudgeOutcome, Error> {
    let references = format_references(responses, cfg.normalize_length, cfg.max_reference_chars);
    let req = ChatCompletionRequest {
        model: cfg.judge.clone(),
        messages: judge_messages(query, &references, cfg.single_source_cap),
        stream: false,
        temperature: Some(0.0),
        max_tokens: None,
    };
    let resp = backend.complete(&req).await?;
    Ok(JudgeOutcome {
        analysis: resp.first_content().to_string(),
        usage: resp.usage.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{ChatCompletionResponse, ChatMessage, Choice};
    use async_trait::async_trait;
    use std::sync::Mutex;

    struct Capturing {
        last_model: Mutex<String>,
    }

    #[async_trait]
    impl ChatBackend for Capturing {
        async fn complete(
            &self,
            req: &ChatCompletionRequest,
        ) -> Result<ChatCompletionResponse, Error> {
            *self.last_model.lock().unwrap() = req.model.clone();
            Ok(ChatCompletionResponse {
                id: "x".into(),
                object: "chat.completion".into(),
                created: 1,
                model: req.model.clone(),
                choices: vec![Choice {
                    index: 0,
                    message: ChatMessage {
                        role: "assistant".into(),
                        content: "Consensus: ...".into(),
                    },
                    finish_reason: Some("stop".into()),
                }],
                usage: Some(Usage {
                    prompt_tokens: 2,
                    completion_tokens: 2,
                    total_tokens: 4,
                }),
            })
        }
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

    #[tokio::test]
    async fn judge_uses_judge_model_and_returns_analysis() {
        let backend = Arc::new(Capturing {
            last_model: Mutex::new(String::new()),
        });
        let out = run_judge(
            Arc::clone(&backend) as Arc<dyn ChatBackend>,
            &agg(),
            "q",
            &["a".into(), "b".into()],
        )
        .await
        .unwrap();
        assert_eq!(out.analysis, "Consensus: ...");
        assert_eq!(*backend.last_model.lock().unwrap(), "b/judge");
        assert_eq!(out.usage.unwrap().total_tokens, 4);
    }
}
