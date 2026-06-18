# Chorus M2 (router gate) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Add a real router gate so a profile can route easy queries to a single model and fuse only the hard ones, using a cheap LLM-judge difficulty scorer with a configurable threshold. This is the M1 design's primary cost lever.

**Architecture:** A new `ClassifierRouter` implements the existing `Router` trait by calling the backend with a small difficulty-scoring prompt and comparing the parsed score to a threshold. The pipeline builds the router per profile from `profile.router` (policy `always_fuse` or `classifier`), so the server is unchanged. Threshold calibration on real traffic is deferred to the deploy track; this lands the implementation.

**Tech Stack:** Same as M1 (chorus-core, tokio, async-trait, wiremock tests). No new dependencies.

**Maps to issues:** the M2 "router classifier policy" roadmap item. Builds on merged M1.

**Conventions:** TDD, conventional commits signed off (`git commit -s`), explicit-path staging, no em/en dashes, clippy pedantic + rustfmt clean.

**Key existing types (from M1, do not redefine):** `chorus_core::router::{Router, RouteDecision, AlwaysFuse}`, `chorus_core::config::{RouterConfig, Profile}`, `chorus_core::backend::ChatBackend`, `chorus_core::schema::{ChatCompletionRequest, ChatMessage}`, `chorus_core::Pipeline`. `RouteDecision` has `Fuse` and `Single`. `Router::decide(&self, &ChatCompletionRequest) -> RouteDecision` (async_trait).

---

## Task 1: Extend RouterConfig for the classifier policy

**Files:** Modify `chorus-core/src/config.rs`.

- [ ] **Step 1: Add fields + defaults + validation, with tests**

Add two fields to `RouterConfig` (keep existing `policy`, `single_model`):

```rust
    #[serde(default)]
    pub classifier_model: Option<String>,
    #[serde(default = "default_threshold")]
    pub threshold: f32,
```

Add `fn default_threshold() -> f32 { 0.5 }` alongside the other `default_*` fns.

In `Config::validate`, for each profile add, after the existing checks:

```rust
            match p.router.policy.as_str() {
                "always_fuse" => {}
                "classifier" => {
                    if p.router.classifier_model.is_none() {
                        return Err(Error::Config(format!(
                            "profile {}: classifier policy requires router.classifier_model",
                            p.name
                        )));
                    }
                }
                other => {
                    return Err(Error::Config(format!(
                        "profile {}: unknown router policy {other}",
                        p.name
                    )));
                }
            }
            if !(0.0..=1.0).contains(&p.router.threshold) {
                return Err(Error::Config(format!(
                    "profile {}: router.threshold {} out of range 0.0..=1.0",
                    p.name, p.router.threshold
                )));
            }
```

Extend `Profile::all_models` (the loop-guard iterator) to also yield `classifier_model` when present:

```rust
    fn all_models(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.router.single_model.as_str())
            .chain(self.router.classifier_model.as_deref())
            .chain(self.panel.members.iter().map(String::as_str))
            .chain(std::iter::once(self.aggregator.judge.as_str()))
            .chain(std::iter::once(self.aggregator.synthesizer.as_str()))
    }
```

Update the existing test helper `profile(...)` so the constructed `RouterConfig` includes `classifier_model: None, threshold: 0.5`. Then add tests:

```rust
    #[test]
    fn classifier_policy_requires_model() {
        let mut p = profile("research", vec!["b/a", "b/b"], 2);
        p.router.policy = "classifier".into();
        p.router.classifier_model = None;
        assert!(matches!(cfg(vec![p]).validate(), Err(Error::Config(_))));
    }

    #[test]
    fn classifier_policy_with_model_passes() {
        let mut p = profile("research", vec!["b/a", "b/b"], 2);
        p.router.policy = "classifier".into();
        p.router.classifier_model = Some("b/cheap".into());
        assert!(cfg(vec![p]).validate().is_ok());
    }

    #[test]
    fn rejects_unknown_router_policy() {
        let mut p = profile("research", vec!["b/a", "b/b"], 2);
        p.router.policy = "magic".into();
        assert!(matches!(cfg(vec![p]).validate(), Err(Error::Config(_))));
    }

    #[test]
    fn rejects_threshold_out_of_range() {
        let mut p = profile("research", vec!["b/a", "b/b"], 2);
        p.router.threshold = 1.5;
        assert!(matches!(cfg(vec![p]).validate(), Err(Error::Config(_))));
    }

    #[test]
    fn rejects_fusion_alias_in_classifier_model() {
        let mut p = profile("research", vec!["b/a", "b/b"], 2);
        p.router.policy = "classifier".into();
        p.router.classifier_model = Some("fusion/research".into());
        assert!(matches!(cfg(vec![p]).validate(), Err(Error::Config(_))));
    }
```

- [ ] **Step 2:** `cargo test -p chorus-core config::tests` -> all pass (old 5 + new 5). `cargo clippy -p chorus-core --all-targets -- -D warnings` and `cargo fmt --all -- --check` clean. Satisfy clippy pedantic (e.g. `#[must_use]`, float-cmp is not triggered by range `contains`).

- [ ] **Step 3:** Commit. `git add chorus-core/src/config.rs` then `git commit -s -m "feat: classifier router config (model, threshold) and validation"`.

---

## Task 2: Difficulty prompt and parser

**Files:** Modify `chorus-core/src/prompts.rs`.

- [ ] **Step 1: Add the difficulty prompt builder and a robust score parser, with tests**

Add to `prompts.rs`:

```rust
/// Messages that ask a cheap model to rate query difficulty as a single number.
#[must_use]
pub fn difficulty_messages(query: &str) -> Vec<ChatMessage> {
    let system = "You are a routing classifier. Rate how hard the user query is to answer \
well, as a single decimal number between 0.0 (trivial, a single capable model answers it \
perfectly) and 1.0 (very hard, benefits from multiple models and synthesis). Reply with ONLY \
the number, nothing else.";
    vec![ChatMessage::system(system), ChatMessage::user(format!("Query:\n{query}"))]
}

/// Parse a difficulty score in 0.0..=1.0 from a model reply, tolerating surrounding text.
/// Returns None if no parseable number is found.
#[must_use]
pub fn parse_difficulty(text: &str) -> Option<f32> {
    // Find the first run that parses as a float; clamp to 0.0..=1.0.
    let mut buf = String::new();
    for ch in text.chars() {
        if ch.is_ascii_digit() || ch == '.' {
            buf.push(ch);
        } else if !buf.is_empty() {
            break;
        }
    }
    buf.parse::<f32>().ok().map(|v| v.clamp(0.0, 1.0))
}

#[cfg(test)]
mod difficulty_tests {
    use super::{difficulty_messages, parse_difficulty};

    #[test]
    fn difficulty_prompt_asks_for_a_single_number() {
        let msgs = difficulty_messages("q");
        assert!(msgs[0].content.contains("single"));
        assert!(msgs[0].content.contains("ONLY the number"));
    }

    #[test]
    fn parses_bare_and_noisy_scores() {
        assert_eq!(parse_difficulty("0.8"), Some(0.8));
        assert_eq!(parse_difficulty("The difficulty is 0.3 overall"), Some(0.3));
        assert_eq!(parse_difficulty("1"), Some(1.0));
        assert_eq!(parse_difficulty("nonsense"), None);
    }

    #[test]
    fn clamps_out_of_range() {
        assert_eq!(parse_difficulty("2.5"), Some(1.0));
    }
}
```

- [ ] **Step 2:** `cargo test -p chorus-core prompts:: difficulty_tests::` (run `cargo test -p chorus-core prompts`) -> pass. clippy + fmt clean.

- [ ] **Step 3:** Commit. `git add chorus-core/src/prompts.rs` then `git commit -s -m "feat: difficulty prompt and score parser for the router"`.

---

## Task 3: ClassifierRouter

**Files:** Modify `chorus-core/src/router.rs`, `chorus-core/src/lib.rs`.

- [ ] **Step 1: Implement ClassifierRouter with fail-open behavior, with tests**

Add to `router.rs` (keep existing `RouteDecision`, `Router`, `AlwaysFuse`):

```rust
use std::sync::Arc;

use crate::backend::ChatBackend;
use crate::prompts::{difficulty_messages, parse_difficulty};
use crate::schema::ChatCompletionRequest;

/// Routes by a cheap LLM-judge difficulty score: fuse when score >= threshold.
/// Fails OPEN to fusion on any backend error or unparseable score.
pub struct ClassifierRouter {
    backend: Arc<dyn ChatBackend>,
    model: String,
    threshold: f32,
}

impl ClassifierRouter {
    #[must_use]
    pub fn new(backend: Arc<dyn ChatBackend>, model: String, threshold: f32) -> Self {
        Self { backend, model, threshold }
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
```

Note: `async_trait` is already imported at the top of `router.rs` from M1. Add the new `use` lines near the existing imports.

Add tests (the existing `router.rs` `tests` module already exists; append a new module or extend):

```rust
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
        async fn complete(&self, _req: &ChatCompletionRequest)
            -> Result<ChatCompletionResponse, Error>
        {
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
```

Add to `chorus-core/src/lib.rs`: extend the router re-export to include `ClassifierRouter`:

```rust
pub use router::{ClassifierRouter, RouteDecision, Router};
```

- [ ] **Step 2:** `cargo test -p chorus-core router` -> all pass (existing + 4 new). clippy + fmt clean (the `score < threshold` float compare is a deliberate threshold check; if clippy `float_cmp` fires it is on equality only, this is `<` so it should be fine; if any pedantic lint fires, satisfy it without changing behavior).

- [ ] **Step 3:** Commit. `git add chorus-core/src/router.rs chorus-core/src/lib.rs` then `git commit -s -m "feat: ClassifierRouter with fail-open LLM-judge difficulty scoring"`.

---

## Task 4: Wire the router per profile in the pipeline

**Files:** Modify `chorus-core/src/pipeline.rs`.

- [ ] **Step 1: Build the router from the profile, with tests**

The M1 `Pipeline` holds a single `router` field set to `AlwaysFuse`. Change it to build the router per profile from `profile.router`. Keep `Pipeline::new(backend)` signature unchanged (so the server needs no change).

Replace the `Pipeline` struct and `new` so it holds only the backend, and add a private router-builder:

```rust
pub struct Pipeline {
    backend: Arc<dyn ChatBackend>,
}

impl Pipeline {
    #[must_use]
    pub fn new(backend: Arc<dyn ChatBackend>) -> Self {
        Self { backend }
    }

    fn router_for(&self, profile: &Profile) -> Arc<dyn Router> {
        match profile.router.policy.as_str() {
            "classifier" => {
                // validated at config load: classifier_model is Some for this policy
                let model = profile
                    .router
                    .classifier_model
                    .clone()
                    .unwrap_or_else(|| profile.router.single_model.clone());
                Arc::new(ClassifierRouter::new(
                    Arc::clone(&self.backend),
                    model,
                    profile.router.threshold,
                ))
            }
            _ => Arc::new(AlwaysFuse),
        }
    }
```

In `run`, replace `self.router.decide(req).await` with:

```rust
        let router = self.router_for(profile);
        let decision = router.decide(req).await;
        tracing::info!(profile = %profile.name, ?decision, "router decision");
        if decision == RouteDecision::Single {
            let mut single = req.clone();
            single.model.clone_from(&profile.router.single_model);
            single.stream = false;
            return self.backend.complete(&single).await;
        }
```

Update imports in `pipeline.rs`: add `ClassifierRouter` to the `use crate::router::...` line (it already imports `AlwaysFuse, RouteDecision, Router`).

The existing M1 pipeline tests use `Pipeline::new(ScriptedBackend)` with an `always_fuse` profile, so they still pass unchanged. Add one classifier test:

```rust
    #[tokio::test]
    async fn classifier_easy_query_routes_to_single_model() {
        // ScriptedBackend answers the difficulty model with a low score, and the
        // single model with a known content. Easy => Single path, no panel/judge/synth.
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
```

Add a `RoutingBackend` test double to the `tests` module that returns a low score for `b/cheap` and "SINGLE ANSWER" for `b/single`:

```rust
    struct RoutingBackend;

    #[async_trait]
    impl ChatBackend for RoutingBackend {
        async fn complete(&self, req: &ChatCompletionRequest)
            -> Result<ChatCompletionResponse, Error>
        {
            let content = match req.model.as_str() {
                "b/cheap" => "0.1",            // low difficulty => Single
                "b/single" => "SINGLE ANSWER",
                other => return Ok(resp(other, "panel", 1)),
            };
            Ok(resp(&req.model, content, 1))
        }
    }
```

(`resp`, `req`, `profile` helpers already exist in the M1 pipeline tests module; reuse them.)

- [ ] **Step 2:** `cargo test -p chorus-core` -> all pass (M1 pipeline tests unchanged + the new classifier route test). clippy + fmt clean.

- [ ] **Step 3:** Commit. `git add chorus-core/src/pipeline.rs` then `git commit -s -m "feat: build router per profile (always_fuse or classifier) in the pipeline"`.

---

## Task 5: Example config, docs, roadmap

**Files:** Modify `config.example.toml`, `docs/design.md`, `ROADMAP.md`.

- [ ] **Step 1: Add a classifier profile example**

Append a second profile to `config.example.toml` showing the classifier policy:

```toml
# A profile that routes easy queries to a single model and fuses only the hard ones.
[[profiles]]
name = "research-routed"

  [profiles.router]
  policy = "classifier"
  classifier_model = "your/cheap-fast-model"
  threshold = 0.5
  single_model = "your/strong-model"

  [profiles.panel]
  members = ["your/model-a", "your/model-b", "your/model-c"]
  min_quorum = 2

  [profiles.aggregator]
  judge = "your/strong-model"
  synthesizer = "your/other-strong-model"
```

- [ ] **Step 2: Update docs/design.md and ROADMAP.md**

In `docs/design.md`, in the Router gate section, change the description so it states the classifier policy is implemented (LLM-judge difficulty score with a configurable threshold, fail-open to fusion), and that threshold calibration on real traffic is the remaining deploy-time step.

In `ROADMAP.md`, change the M2 router line from not-started to in-progress/done as appropriate: mark "Router policy: a cheap difficulty classifier ... with a per-deployment threshold; fail-open to fusion" as done, and leave "Threshold calibration on real traffic" as not started (needs deploy).

- [ ] **Step 3:** No-dash check on all three files. Commit. `git add config.example.toml docs/design.md ROADMAP.md` then `git commit -s -m "docs: classifier router example, design and roadmap updates"`.

---

## Task 6: Verify and finish

- [ ] **Step 1:** Full verification:
  - `cargo test --workspace` -> all pass.
  - `cargo clippy --workspace --all-targets -- -D warnings` -> clean.
  - `cargo fmt --all -- --check` -> clean.
  - `cargo deny check` -> ok (no new deps, should remain ok).
  - Dash check: `git diff 03c6ace HEAD | grep -nP '[\x{2014}\x{2013}]'` must be empty.

- [ ] **Step 2:** Do NOT push or open a PR here; the controller runs the final whole-branch review then finishes the branch.

---

## Self-Review

**Spec coverage:** Router config (Task 1), difficulty prompt+parser (Task 2), ClassifierRouter with fail-open (Task 3), per-profile wiring in the pipeline preserving the M1 server interface (Task 4), example/docs (Task 5), verification (Task 6). Threshold calibration and the sovereignty benchmark are explicitly deferred to the deploy track (they need live inference), and noted in the roadmap.

**Placeholders:** none; all steps carry complete code.

**Type consistency:** `RouterConfig` gains `classifier_model: Option<String>` and `threshold: f32`, referenced consistently in config validation, `ClassifierRouter::new`, and `Pipeline::router_for`. `ClassifierRouter::new(Arc<dyn ChatBackend>, String, f32)` is the single constructor used in Task 3 tests and Task 4 wiring. `Router`/`RouteDecision` are the M1 types, unchanged. `Pipeline::new(Arc<dyn ChatBackend>)` signature is preserved so `chorus-server` needs no change.

**Server impact:** none. `Pipeline::new(backend)` is unchanged; routing is now per-profile inside `run`. The existing integration tests (always_fuse profile) stay green.
