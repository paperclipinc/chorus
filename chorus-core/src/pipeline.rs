//! Orchestrate router -> panel -> judge -> synthesis for one request.

use std::sync::Arc;

use crate::backend::ChatBackend;
use crate::config::Profile;
use crate::error::Error;
use crate::judge::run_judge;
use crate::panel::run_panel;
use crate::router::{AlwaysFuse, ClassifierRouter, RouteDecision, Router};
use crate::schema::{ChatCompletionRequest, ChatCompletionResponse, ChatMessage, Choice};
use crate::synthesis::run_synthesis;
use crate::usage::UsageAccumulator;

/// Build a degraded fusion response from the strongest available panel answer
/// when synthesis fails. The longest non-empty response is used as a
/// deterministic "most substantive" proxy for the best answer. Usage is left
/// `None` here because the panel and judge token costs are aggregated by the
/// caller. Returns `None` only if no panel answer is usable, in which case the
/// caller surfaces the synthesis error.
fn degraded_from_panel(responses: &[String]) -> Option<ChatCompletionResponse> {
    let best = responses
        .iter()
        .filter(|r| !r.trim().is_empty())
        .max_by_key(|r| r.len())?;
    Some(ChatCompletionResponse {
        id: "chorus-degraded".to_string(),
        object: "chat.completion".to_string(),
        created: 0,
        // Set to the fusion alias by the caller alongside aggregated usage.
        model: String::new(),
        choices: vec![Choice {
            index: 0,
            message: ChatMessage {
                role: "assistant".to_string(),
                content: best.clone(),
            },
            finish_reason: Some("stop".to_string()),
        }],
        usage: None,
    })
}

/// End-to-end `MoA` pipeline: router gate -> panel -> judge -> synthesis.
#[must_use]
pub struct Pipeline {
    backend: Arc<dyn ChatBackend>,
}

impl Pipeline {
    /// Construct the pipeline.  The router is built per-profile inside [`run`](Self::run).
    pub fn new(backend: Arc<dyn ChatBackend>) -> Self {
        Self { backend }
    }

    /// Choose a router based on `profile.router.policy`.
    ///
    /// * `"classifier"` -- [`ClassifierRouter`] using `classifier_model` (validated `Some`
    ///   by config) and the configured threshold.
    /// * anything else (including `"always_fuse"`) -- [`AlwaysFuse`].
    fn router_for(&self, profile: &Profile) -> Arc<dyn Router> {
        match profile.router.policy.as_str() {
            "classifier" => {
                if let Some(model) = profile.router.classifier_model.clone() {
                    Arc::new(ClassifierRouter::new(
                        Arc::clone(&self.backend),
                        model,
                        profile.router.threshold,
                    ))
                } else {
                    tracing::warn!(
                        profile = %profile.name,
                        "classifier policy without classifier_model; falling back to always-fuse"
                    );
                    Arc::new(AlwaysFuse)
                }
            }
            _ => Arc::new(AlwaysFuse),
        }
    }

    /// Run one request through the full pipeline for the given [`Profile`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::Quorum`] if fewer panel members succeed than `profile.panel.min_quorum`.
    /// Returns [`Error::Synthesis`] only if the synthesizer fails (after its retry) and no panel
    /// answer is available to degrade to; otherwise a synthesizer failure degrades to a panel
    /// answer rather than erroring.
    /// Returns [`Error::Backend`] if the single-model forward fails.
    pub async fn run(
        &self,
        profile: &Profile,
        req: &ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse, Error> {
        // Router gate: build the router from the profile, then decide.
        let router = self.router_for(profile);
        let decision = router.decide(req).await;
        tracing::info!(profile = %profile.name, ?decision, "router decision");
        let branch = match decision {
            RouteDecision::Fuse => "fuse",
            RouteDecision::Single => "single",
        };
        metrics::counter!(
            "chorus_router_decisions_total",
            "profile" => profile.name.clone(),
            "decision" => branch,
        )
        .increment(1);
        if decision == RouteDecision::Single {
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

        // Synthesis, with graceful degradation to the strongest panel answer on
        // failure. run_synthesis already retries a transient synthesizer error
        // once; if it still fails, a single flaky synthesizer must not 502 the
        // whole fusion when the panel produced usable answers (issue #32).
        let mut resp = match run_synthesis(
            Arc::clone(&self.backend),
            &profile.aggregator,
            &query,
            &panel.responses,
            &analysis,
            req.max_tokens,
        )
        .await
        {
            Ok(r) => r,
            Err(e) => match degraded_from_panel(&panel.responses) {
                Some(r) => {
                    tracing::warn!(error = %e, "synthesis failed after retry; degrading to best panel answer");
                    metrics::counter!(
                        "chorus_synthesis_degraded_total",
                        "profile" => profile.name.clone(),
                    )
                    .increment(1);
                    r
                }
                None => return Err(e),
            },
        };
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
            max_tokens: None,
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

    /// Judge succeeds, panel succeeds, but the synthesizer always fails to decode.
    struct SynthFailsBackend;

    #[async_trait]
    impl ChatBackend for SynthFailsBackend {
        async fn complete(
            &self,
            req: &ChatCompletionRequest,
        ) -> Result<ChatCompletionResponse, Error> {
            match req.model.as_str() {
                "b/judge" => Ok(resp(&req.model, "Consensus: ...", 1)),
                "b/syn" => Err(Error::Backend(
                    "decode: error decoding response body".into(),
                )),
                other => Ok(resp(other, &format!("panel:{other}"), 1)),
            }
        }
    }

    #[tokio::test]
    async fn degrades_to_panel_answer_when_synthesis_fails() {
        // A flaky synthesizer must not 502 the whole fusion: the pipeline falls
        // back to the strongest panel answer instead (issue #32).
        let p = Pipeline::new(Arc::new(SynthFailsBackend));
        let out = p
            .run(&profile(), &req())
            .await
            .expect("synthesis failure should degrade, not error");
        assert!(
            out.first_content().starts_with("panel:"),
            "expected a panel answer, got {:?}",
            out.first_content()
        );
        assert_eq!(out.model, "fusion/research");
        // Panel (3) + judge (1) calls are still accounted; the degraded answer
        // carries no synthesizer usage of its own.
        assert_eq!(out.usage.unwrap().total_tokens, 8);
    }

    /// Returns a low difficulty score for `b/cheap` and "SINGLE ANSWER" for `b/single`.
    /// Any other model gets a generic panel response.
    struct RoutingBackend;

    #[async_trait]
    impl ChatBackend for RoutingBackend {
        async fn complete(
            &self,
            req: &ChatCompletionRequest,
        ) -> Result<ChatCompletionResponse, Error> {
            let content = match req.model.as_str() {
                "b/cheap" => "0.1", // low difficulty => Single
                "b/single" => "SINGLE ANSWER",
                other => return Ok(resp(other, "panel", 1)),
            };
            Ok(resp(&req.model, content, 1))
        }
    }

    #[tokio::test]
    async fn classifier_easy_query_routes_to_single_model() {
        // RoutingBackend answers the difficulty model with a low score, so the
        // classifier routes to Single; no panel/judge/synth is invoked.
        let p = {
            let mut p = profile();
            p.router.policy = "classifier".into();
            p.router.classifier_model = Some("b/cheap".into());
            p.router.single_model = "b/single".into();
            p.router.threshold = 0.5;
            p
        };
        let pipe = Pipeline::new(Arc::new(RoutingBackend));
        let out = pipe.run(&p, &req()).await.unwrap();
        assert_eq!(out.first_content(), "SINGLE ANSWER");
    }
}
