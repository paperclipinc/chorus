//! Orchestrate router -> panel -> judge -> synthesis for one request.

use std::sync::Arc;

use crate::backend::ChatBackend;
use crate::config::Profile;
use crate::error::Error;
use crate::judge::run_judge;
use crate::panel::run_panel;
use crate::router::{AlwaysFuse, RouteDecision, Router};
use crate::schema::{ChatCompletionRequest, ChatCompletionResponse};
use crate::synthesis::run_synthesis;
use crate::usage::UsageAccumulator;

/// End-to-end `MoA` pipeline: router gate -> panel -> judge -> synthesis.
#[must_use]
pub struct Pipeline {
    backend: Arc<dyn ChatBackend>,
    router: Arc<dyn Router>,
}

impl Pipeline {
    /// Construct with the M1 [`AlwaysFuse`] router.
    pub fn new(backend: Arc<dyn ChatBackend>) -> Self {
        Self {
            backend,
            router: Arc::new(AlwaysFuse),
        }
    }

    /// Run one request through the full pipeline for the given [`Profile`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::Quorum`] if fewer panel members succeed than `profile.panel.min_quorum`.
    /// Returns [`Error::Synthesis`] if the synthesizer call fails.
    /// Returns [`Error::Backend`] if the single-model forward fails.
    pub async fn run(
        &self,
        profile: &Profile,
        req: &ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse, Error> {
        // Router gate. `AlwaysFuse` never returns `Single`, but the branch is
        // here so a different router can be injected without touching the rest.
        if self.router.decide(req).await == RouteDecision::Single {
            let mut single = req.clone();
            single.model.clone_from(&profile.router.single_model);
            single.stream = false;
            return self.backend.complete(&single).await;
        }

        let mut acc = UsageAccumulator::default();
        let query = req.last_user_text().to_string();

        // Panel.
        let panel = run_panel(Arc::clone(&self.backend), req, &profile.panel).await?;
        acc.add(Some(&panel.usage));

        // Judge, with graceful degradation to an empty analysis on failure.
        let analysis = match run_judge(
            Arc::clone(&self.backend),
            &profile.aggregator,
            &query,
            &panel.responses,
        )
        .await
        {
            Ok(j) => {
                acc.add(j.usage.as_ref());
                j.analysis
            }
            Err(e) => {
                tracing::warn!(error = %e, "judge failed; synthesizing over raw responses");
                String::new()
            }
        };

        // Synthesis.
        let mut resp = run_synthesis(
            Arc::clone(&self.backend),
            &profile.aggregator,
            &query,
            &panel.responses,
            &analysis,
        )
        .await?;
        acc.add(resp.usage.as_ref());

        // Present as the fusion alias, with aggregated usage.
        resp.model = format!("fusion/{}", profile.name);
        resp.usage = Some(acc.into_usage());
        Ok(resp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AggregatorConfig, PanelConfig, Profile, RouterConfig};
    use crate::schema::{ChatCompletionResponse, ChatMessage, Choice, Usage};
    use async_trait::async_trait;

    /// Routes by model id so we can assert which stage called which model.
    struct ScriptedBackend;

    fn resp(model: &str, content: &str, tokens: u32) -> ChatCompletionResponse {
        ChatCompletionResponse {
            id: "x".into(),
            object: "chat.completion".into(),
            created: 1,
            model: model.into(),
            choices: vec![Choice {
                index: 0,
                message: ChatMessage {
                    role: "assistant".into(),
                    content: content.into(),
                },
                finish_reason: Some("stop".into()),
            }],
            usage: Some(Usage {
                prompt_tokens: tokens,
                completion_tokens: tokens,
                total_tokens: tokens * 2,
            }),
        }
    }

    #[async_trait]
    impl ChatBackend for ScriptedBackend {
        async fn complete(
            &self,
            req: &ChatCompletionRequest,
        ) -> Result<ChatCompletionResponse, Error> {
            let content = match req.model.as_str() {
                "b/syn" => "FINAL ANSWER",
                "b/judge" => "Consensus: ...",
                other => return Ok(resp(other, &format!("panel:{other}"), 1)),
            };
            Ok(resp(&req.model, content, 1))
        }
    }

    fn profile() -> Profile {
        Profile {
            name: "research".into(),
            router: RouterConfig {
                policy: "always_fuse".into(),
                single_model: "b/single".into(),
                classifier_model: None,
                threshold: 0.5,
            },
            panel: PanelConfig {
                members: vec!["b/a".into(), "b/b".into(), "b/c".into()],
                self_moa: false,
                samples: 3,
                min_quorum: 2,
                timeout_ms: 1_000,
            },
            aggregator: AggregatorConfig {
                judge: "b/judge".into(),
                synthesizer: "b/syn".into(),
                anonymize_sources: true,
                normalize_length: true,
                single_source_cap: true,
                layers: 1,
                max_reference_chars: 8_000,
            },
            tools: vec![],
        }
    }

    fn req() -> ChatCompletionRequest {
        ChatCompletionRequest {
            model: "fusion/research".into(),
            messages: vec![ChatMessage::user("what is X")],
            stream: false,
            temperature: None,
        }
    }

    #[tokio::test]
    async fn full_pipeline_returns_synthesized_answer_with_aggregated_usage() {
        let p = Pipeline::new(Arc::new(ScriptedBackend));
        let out = p.run(&profile(), &req()).await.unwrap();
        assert_eq!(out.first_content(), "FINAL ANSWER");
        assert_eq!(out.model, "fusion/research");
        // 3 panel + 1 judge + 1 synth = 5 calls, each total_tokens 2 => 10.
        assert_eq!(out.usage.unwrap().total_tokens, 10);
    }

    struct JudgeFailsBackend;

    #[async_trait]
    impl ChatBackend for JudgeFailsBackend {
        async fn complete(
            &self,
            req: &ChatCompletionRequest,
        ) -> Result<ChatCompletionResponse, Error> {
            match req.model.as_str() {
                "b/judge" => Err(Error::Backend("judge down".into())),
                "b/syn" => Ok(resp(&req.model, "FINAL DESPITE NO JUDGE", 1)),
                other => Ok(resp(other, "panel", 1)),
            }
        }
    }

    #[tokio::test]
    async fn degrades_when_judge_fails() {
        let p = Pipeline::new(Arc::new(JudgeFailsBackend));
        let out = p.run(&profile(), &req()).await.unwrap();
        assert_eq!(out.first_content(), "FINAL DESPITE NO JUDGE");
    }
}
