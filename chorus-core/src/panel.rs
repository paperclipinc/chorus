//! Concurrent panel fan-out with partial-failure quorum and self-MoA sampling.

use std::sync::Arc;
use std::time::Duration;

use tokio::task::JoinSet;
use tokio::time::timeout;

use crate::backend::ChatBackend;
use crate::config::PanelConfig;
use crate::error::Error;
use crate::schema::{ChatCompletionRequest, Usage};
use crate::usage::UsageAccumulator;

/// The outcome of a successful panel run.
#[derive(Debug)]
pub struct PanelOutcome {
    /// Survivor answer contents, anonymized by position downstream.
    pub responses: Vec<String>,
    /// Aggregated token usage across all surviving panel members.
    pub usage: Usage,
}

/// Build the per-call request for member `idx`.
///
/// In self-MoA mode the temperature is varied across samples to increase response diversity.
fn member_request(
    base: &ChatCompletionRequest,
    model: &str,
    self_moa: bool,
    idx: usize,
) -> ChatCompletionRequest {
    let mut req = base.clone();
    req.model = model.to_string();
    req.stream = false;
    if self_moa {
        // Vary temperature across samples of the single model for diversity.
        #[allow(clippy::cast_precision_loss)]
        let temp = (0.3 + 0.2 * (idx as f32)).min(2.0);
        req.temperature = Some(temp);
    }
    req
}

/// The list of `(model, idx)` calls this panel will make.
///
/// In self-MoA mode `members[0]` is called `samples` times with varied temperature.
/// In normal mode each member is called once.
fn member_calls(cfg: &PanelConfig) -> Vec<(String, usize)> {
    if cfg.self_moa {
        let model = cfg.members.first().cloned().unwrap_or_default();
        (0..cfg.samples).map(|i| (model.clone(), i)).collect()
    } else {
        cfg.members
            .iter()
            .cloned()
            .enumerate()
            .map(|(i, m)| (m, i))
            .collect()
    }
}

/// Fan out to all panel members concurrently, apply a per-call timeout, drop
/// failed or timed-out members, aggregate usage, and return [`Error::Quorum`]
/// if fewer than `min_quorum` members survive.
///
/// # Errors
///
/// Returns [`Error::Quorum`] when fewer than `cfg.min_quorum` panel members
/// return a successful response within the configured timeout.
pub async fn run_panel(
    backend: Arc<dyn ChatBackend>,
    req: &ChatCompletionRequest,
    cfg: &PanelConfig,
) -> Result<PanelOutcome, Error> {
    let calls = member_calls(cfg);
    let per_call = Duration::from_millis(cfg.timeout_ms);
    let self_moa = cfg.self_moa;

    let mut set = JoinSet::new();
    for (model, idx) in calls {
        let backend = Arc::clone(&backend);
        let member_req = member_request(req, &model, self_moa, idx);
        set.spawn(async move {
            match timeout(per_call, backend.complete(&member_req)).await {
                Ok(Ok(resp)) => Some(resp),
                _ => None, // timeout or error: drop this member
            }
        });
    }

    let mut responses = Vec::new();
    let mut acc = UsageAccumulator::default();
    while let Some(joined) = set.join_next().await {
        if let Ok(Some(resp)) = joined {
            acc.add(resp.usage.as_ref());
            responses.push(resp.first_content().to_string());
        }
    }

    if responses.len() < cfg.min_quorum {
        return Err(Error::Quorum {
            got: responses.len(),
            needed: cfg.min_quorum,
        });
    }

    Ok(PanelOutcome {
        responses,
        usage: acc.into_usage(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{ChatCompletionResponse, ChatMessage, Choice};
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A mock backend that returns a fixed body, optionally failing the first N calls.
    struct MockBackend {
        fail_first: usize,
        seen: AtomicUsize,
    }

    fn resp(content: &str) -> ChatCompletionResponse {
        ChatCompletionResponse {
            id: "x".into(),
            object: "chat.completion".into(),
            created: 1,
            model: "m".into(),
            choices: vec![Choice {
                index: 0,
                message: ChatMessage {
                    role: "assistant".into(),
                    content: content.into(),
                },
                finish_reason: Some("stop".into()),
            }],
            usage: Some(Usage {
                prompt_tokens: 1,
                completion_tokens: 1,
                total_tokens: 2,
            }),
        }
    }

    #[async_trait]
    impl ChatBackend for MockBackend {
        async fn complete(
            &self,
            req: &ChatCompletionRequest,
        ) -> Result<ChatCompletionResponse, Error> {
            let n = self.seen.fetch_add(1, Ordering::SeqCst);
            if n < self.fail_first {
                return Err(Error::Backend("boom".into()));
            }
            Ok(resp(&format!("answer-from-{}", req.model)))
        }
    }

    fn base_req() -> ChatCompletionRequest {
        ChatCompletionRequest {
            model: "fusion/research".into(),
            messages: vec![ChatMessage::user("hi")],
            stream: false,
            temperature: None,
        }
    }

    fn cfg(members: &[&str], quorum: usize, self_moa: bool) -> PanelConfig {
        PanelConfig {
            members: members.iter().map(|s| (*s).to_string()).collect(),
            self_moa,
            samples: 3,
            min_quorum: quorum,
            timeout_ms: 1_000,
        }
    }

    #[tokio::test]
    async fn all_members_succeed() {
        let backend = Arc::new(MockBackend {
            fail_first: 0,
            seen: AtomicUsize::new(0),
        });
        let out = run_panel(backend, &base_req(), &cfg(&["a", "b", "c"], 2, false))
            .await
            .unwrap();
        assert_eq!(out.responses.len(), 3);
        assert_eq!(out.usage.total_tokens, 6);
    }

    #[tokio::test]
    async fn proceeds_at_quorum_with_partial_failure() {
        let backend = Arc::new(MockBackend {
            fail_first: 1,
            seen: AtomicUsize::new(0),
        });
        let out = run_panel(backend, &base_req(), &cfg(&["a", "b", "c"], 2, false))
            .await
            .unwrap();
        assert_eq!(out.responses.len(), 2);
    }

    #[tokio::test]
    async fn fails_below_quorum() {
        let backend = Arc::new(MockBackend {
            fail_first: 2,
            seen: AtomicUsize::new(0),
        });
        let err = run_panel(backend, &base_req(), &cfg(&["a", "b", "c"], 2, false))
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Quorum { got: 1, needed: 2 }));
    }

    #[tokio::test]
    async fn self_moa_samples_one_model() {
        let backend = Arc::new(MockBackend {
            fail_first: 0,
            seen: AtomicUsize::new(0),
        });
        let out = run_panel(backend, &base_req(), &cfg(&["only"], 1, true))
            .await
            .unwrap();
        assert_eq!(out.responses.len(), 3); // samples, not members
        assert!(out.responses.iter().all(|r| r == "answer-from-only"));
    }
}
