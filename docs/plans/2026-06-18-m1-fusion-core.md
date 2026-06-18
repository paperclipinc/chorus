# Chorus M1 (fusion core) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the chorus OpenAI-compatible Mixture-of-Agents gateway (the `AlwaysFuse`, single-layer, hardened-synthesis core) so that one request to `model: "fusion/<profile>"` fans out to a panel, judges, synthesizes, and returns one OpenAI-compatible answer with aggregated usage.

**Architecture:** Two crates. `chorus-core` is the backend-agnostic engine (schema, backend trait, router, panel, judge, synthesis, pipeline, config). `chorus-server` is an axum HTTP service exposing the OpenAI surface, a concurrency limiter, metrics, and streaming. Every model call goes to one configured OpenAI-compatible backend via the `ChatBackend` trait, so tests run against a `wiremock` mock with no network.

**Tech Stack:** Rust edition 2024, tokio 1.52, axum 0.8, reqwest 0.13 (rustls), serde 1, thiserror 2, async-trait 0.1, figment 0.10, tracing 0.3-subscriber, metrics 0.24 + metrics-exporter-prometheus 0.18, async-stream; tests with wiremock 0.6 and insta 1.

**Maps to issues:** #1-#13 in `paperclipinc/chorus`. (#14 benchmark wiring and the mono gitops deployment are a separate plan, different repo and toolchain.)

**Conventions (from CLAUDE.md):** TDD (failing test first, in the same commit as the code). Conventional commits, signed off (`git commit -s`). Stage explicit paths, never `git add -A`. No em or en dashes anywhere. `#![forbid(unsafe_code)]`. clippy pedantic and rustfmt clean.

---

## File Structure

```
chorus/
  Cargo.toml                    # workspace: members, shared deps, lints
  rust-toolchain.toml           # stable channel, rustfmt + clippy
  rustfmt.toml                  # formatting config
  deny.toml                     # cargo-deny licenses/advisories/bans
  config.example.toml           # a runnable example profile
  chorus-core/
    Cargo.toml
    src/
      lib.rs                    # module decls + re-exports
      error.rs                  # Error enum (thiserror)
      schema.rs                 # OpenAI request/response/usage types
      backend.rs                # ChatBackend trait + OpenAiBackend (reqwest)
      config.rs                 # Config/Profile/* + validation (loop guard, quorum)
      router.rs                 # Router trait + AlwaysFuse
      panel.rs                  # concurrent fan-out, quorum, self_moa
      prompts.rs                # anonymization, length normalization, prompt builders
      judge.rs                  # structured-analysis judge call
      synthesis.rs              # hardened synthesis call
      usage.rs                  # usage aggregation helper
      pipeline.rs               # router -> panel -> judge -> synthesis
  chorus-server/
    Cargo.toml
    src/
      main.rs                   # config load, backend build, serve, shutdown
      state.rs                  # AppState
      error.rs                  # map core Error -> OpenAI error JSON + HTTP status
      app.rs                    # axum Router builder + middleware
      handlers.rs               # /v1/chat/completions, /v1/models, /healthz, /metrics
      sse.rs                    # streaming synthesis SSE
      metrics.rs                # prometheus recorder install + handle
  tests/                        # (chorus-server) integration tests with wiremock
```

Each `chorus-core` module has one responsibility and depends only on `schema` + `error` (plus `backend` for the call-making modules). The server depends on `chorus-core` only through its public API.

---

## Task 1: Workspace scaffolding (issue #1)

**Files:**
- Create: `Cargo.toml`, `rust-toolchain.toml`, `rustfmt.toml`, `deny.toml`
- Create: `chorus-core/Cargo.toml`, `chorus-core/src/lib.rs`
- Create: `chorus-server/Cargo.toml`, `chorus-server/src/main.rs`

- [ ] **Step 1: Write the workspace manifest**

Create `Cargo.toml`:

```toml
[workspace]
members = ["chorus-core", "chorus-server"]
resolver = "3"

[workspace.package]
edition = "2024"
license = "Apache-2.0"
version = "0.0.0"
repository = "https://github.com/paperclipinc/chorus"

[workspace.lints.rust]
unsafe_code = "forbid"

[workspace.lints.clippy]
pedantic = { level = "warn", priority = -1 }
module_name_repetitions = "allow"

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
anyhow = "1"
tokio = { version = "1.52", features = ["rt-multi-thread", "macros", "sync", "time"] }
async-trait = "0.1"
reqwest = { version = "0.13", default-features = false, features = ["json", "rustls-tls", "stream"] }
futures = "0.3"
tracing = "0.1"
```

- [ ] **Step 2: Write `rust-toolchain.toml`, `rustfmt.toml`, `deny.toml`**

`rust-toolchain.toml`:

```toml
[toolchain]
channel = "stable"
components = ["rustfmt", "clippy"]
```

`rustfmt.toml`:

```toml
edition = "2024"
max_width = 100
```

`deny.toml`:

```toml
[advisories]
version = 2
yanked = "deny"

[licenses]
version = 2
allow = ["Apache-2.0", "MIT", "BSD-3-Clause", "ISC", "Unicode-3.0", "Zlib"]

[bans]
multiple-versions = "warn"
```

- [ ] **Step 3: Write the core crate manifest and an empty lib**

`chorus-core/Cargo.toml`:

```toml
[package]
name = "chorus-core"
edition.workspace = true
license.workspace = true
version.workspace = true
repository.workspace = true

[lints]
workspace = true

[dependencies]
serde.workspace = true
serde_json.workspace = true
thiserror.workspace = true
tokio.workspace = true
async-trait.workspace = true
reqwest.workspace = true
futures.workspace = true
tracing.workspace = true

[dev-dependencies]
wiremock = "0.6"
insta = "1"
tokio = { version = "1.52", features = ["rt-multi-thread", "macros", "sync", "time", "test-util"] }
```

`chorus-core/src/lib.rs`:

```rust
//! chorus-core: the backend-agnostic Mixture-of-Agents engine.

pub mod error;
pub mod schema;

pub use error::Error;
```

- [ ] **Step 4: Write the server crate manifest and a stub main**

`chorus-server/Cargo.toml`:

```toml
[package]
name = "chorus-server"
edition.workspace = true
license.workspace = true
version.workspace = true
repository.workspace = true

[lints]
workspace = true

[dependencies]
chorus-core = { path = "../chorus-core" }
serde.workspace = true
serde_json.workspace = true
anyhow.workspace = true
tokio = { version = "1.52", features = ["rt-multi-thread", "macros", "sync", "time", "signal"] }
tracing.workspace = true
tracing-subscriber = { version = "0.3", features = ["env-filter", "json"] }
axum = "0.8"
tower-http = { version = "0.7", features = ["trace"] }
figment = { version = "0.10", features = ["toml", "env"] }
metrics = "0.24"
metrics-exporter-prometheus = { version = "0.18", default-features = false }
async-stream = "0.3"
futures.workspace = true

[dev-dependencies]
wiremock = "0.6"
reqwest = { version = "0.13", default-features = false, features = ["json", "rustls-tls", "stream"] }
```

`chorus-server/src/main.rs`:

```rust
fn main() {
    println!("chorus-server");
}
```

- [ ] **Step 5: Verify the workspace builds and lints**

Run: `cargo build --workspace && cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected: builds clean, no clippy warnings, formatting OK.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml rust-toolchain.toml rustfmt.toml deny.toml chorus-core/Cargo.toml chorus-core/src/lib.rs chorus-server/Cargo.toml chorus-server/src/main.rs
git commit -s -m "feat: cargo workspace scaffolding for chorus-core and chorus-server"
```

---

## Task 2: Error type (part of issues #2, #3)

**Files:**
- Create: `chorus-core/src/error.rs`

- [ ] **Step 1: Write the failing test**

Append to `chorus-core/src/error.rs`:

```rust
//! The single error type for the engine.

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("backend request failed: {0}")]
    Backend(String),
    #[error("backend timed out")]
    Timeout,
    #[error("quorum not met: {got} of {needed} panel members returned")]
    Quorum { got: usize, needed: usize },
    #[error("synthesis failed: {0}")]
    Synthesis(String),
    #[error("unknown profile: {0}")]
    UnknownProfile(String),
    #[error("invalid model alias: {0}")]
    InvalidModel(String),
    #[error("config error: {0}")]
    Config(String),
}

#[cfg(test)]
mod tests {
    use super::Error;

    #[test]
    fn quorum_error_renders_counts() {
        let e = Error::Quorum { got: 1, needed: 2 };
        assert_eq!(e.to_string(), "quorum not met: 1 of 2 panel members returned");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p chorus-core error::tests::quorum_error_renders_counts`
Expected: FAIL (module not declared in lib.rs yet, compile error).

- [ ] **Step 3: Make it compile**

`lib.rs` already declares `pub mod error;` from Task 1. Confirm it does; if not, add it.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p chorus-core error::tests::quorum_error_renders_counts`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add chorus-core/src/error.rs
git commit -s -m "feat: core Error type"
```

---

## Task 3: OpenAI schema types (issue #2)

**Files:**
- Create: `chorus-core/src/schema.rs`
- Modify: `chorus-core/src/lib.rs` (re-export)

- [ ] **Step 1: Write the failing test**

Create `chorus-core/src/schema.rs`:

```rust
//! Hand-owned OpenAI chat-completion types.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self { role: "system".into(), content: content.into() }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self { role: "user".into(), content: content.into() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub stream: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub temperature: Option<f32>,
}

impl ChatCompletionRequest {
    /// The last user message content, used as the query for judge and synthesis.
    pub fn last_user_text(&self) -> &str {
        self.messages
            .iter()
            .rev()
            .find(|m| m.role == "user")
            .map_or("", |m| m.content.as_str())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Choice {
    pub index: u32,
    pub message: ChatMessage,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<Choice>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

impl ChatCompletionResponse {
    /// Convenience: the assistant content of the first choice.
    pub fn first_content(&self) -> &str {
        self.choices.first().map_or("", |c| c.message.content.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_defaults_stream_false() {
        let json = r#"{"model":"m","messages":[{"role":"user","content":"hi"}]}"#;
        let req: ChatCompletionRequest = serde_json::from_str(json).unwrap();
        assert!(!req.stream);
        assert_eq!(req.last_user_text(), "hi");
    }

    #[test]
    fn response_roundtrips_and_reads_first_content() {
        let json = r#"{"id":"a","object":"chat.completion","created":1,"model":"m",
            "choices":[{"index":0,"message":{"role":"assistant","content":"yo"},"finish_reason":"stop"}],
            "usage":{"prompt_tokens":3,"completion_tokens":1,"total_tokens":4}}"#;
        let resp: ChatCompletionResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.first_content(), "yo");
        assert_eq!(resp.usage.unwrap().total_tokens, 4);
    }
}
```

- [ ] **Step 2: Add the module and run the failing test**

In `chorus-core/src/lib.rs` confirm `pub mod schema;` exists (added in Task 1). Add re-exports:

```rust
pub use schema::{
    ChatCompletionRequest, ChatCompletionResponse, ChatMessage, Choice, Usage,
};
```

Run: `cargo test -p chorus-core schema::tests`
Expected: PASS (these are self-contained).

- [ ] **Step 3: Commit**

```bash
git add chorus-core/src/schema.rs chorus-core/src/lib.rs
git commit -s -m "feat: OpenAI chat-completion schema types"
```

---

## Task 4: Backend trait and OpenAiBackend (issue #3)

**Files:**
- Create: `chorus-core/src/backend.rs`
- Modify: `chorus-core/src/lib.rs`

- [ ] **Step 1: Write the failing test (against a wiremock backend)**

Create `chorus-core/src/backend.rs`:

```rust
//! The backend abstraction: one OpenAI-compatible upstream.

use std::time::Duration;

use async_trait::async_trait;

use crate::error::Error;
use crate::schema::{ChatCompletionRequest, ChatCompletionResponse};

/// Anything chorus can send a chat-completion request to.
#[async_trait]
pub trait ChatBackend: Send + Sync {
    async fn complete(&self, req: &ChatCompletionRequest)
        -> Result<ChatCompletionResponse, Error>;
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
    pub fn new(base_url: impl Into<String>, api_key: impl Into<String>, timeout: Duration)
        -> Result<Self, Error>
    {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| Error::Backend(e.to_string()))?;
        Ok(Self { client, base_url: base_url.into(), api_key: api_key.into() })
    }
}

#[async_trait]
impl ChatBackend for OpenAiBackend {
    async fn complete(&self, req: &ChatCompletionRequest)
        -> Result<ChatCompletionResponse, Error>
    {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let resp = self
            .client
            .post(url)
            .bearer_auth(&self.api_key)
            .json(req)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() { Error::Timeout } else { Error::Backend(e.to_string()) }
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
```

- [ ] **Step 2: Declare the module and run the failing test**

Add to `chorus-core/src/lib.rs`:

```rust
pub mod backend;
pub use backend::{ChatBackend, OpenAiBackend};
```

Run: `cargo test -p chorus-core backend::tests`
Expected: PASS (wiremock serves the mock; no real network).

- [ ] **Step 3: Commit**

```bash
git add chorus-core/src/backend.rs chorus-core/src/lib.rs
git commit -s -m "feat: ChatBackend trait and OpenAiBackend"
```

---

## Task 5: Config and validation (issue #4)

**Files:**
- Create: `chorus-core/src/config.rs`
- Modify: `chorus-core/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

Create `chorus-core/src/config.rs`:

```rust
//! Typed configuration and validation.

use serde::Deserialize;

use crate::error::Error;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub backend: BackendConfig,
    pub profiles: Vec<Profile>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    pub bind: String,
    #[serde(default = "default_max_concurrent")]
    pub max_concurrent_requests: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BackendConfig {
    pub base_url: String,
    pub api_key_env: String,
    #[serde(default = "default_backend_timeout_ms")]
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Profile {
    pub name: String,
    pub router: RouterConfig,
    pub panel: PanelConfig,
    pub aggregator: AggregatorConfig,
    #[serde(default)]
    pub tools: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RouterConfig {
    #[serde(default = "default_router_policy")]
    pub policy: String,
    pub single_model: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PanelConfig {
    pub members: Vec<String>,
    #[serde(default)]
    pub self_moa: bool,
    #[serde(default = "default_samples")]
    pub samples: usize,
    pub min_quorum: usize,
    #[serde(default = "default_panel_timeout_ms")]
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AggregatorConfig {
    pub judge: String,
    pub synthesizer: String,
    #[serde(default = "default_true")]
    pub anonymize_sources: bool,
    #[serde(default = "default_true")]
    pub normalize_length: bool,
    #[serde(default = "default_true")]
    pub single_source_cap: bool,
    #[serde(default = "default_layers")]
    pub layers: usize,
    #[serde(default = "default_max_reference_chars")]
    pub max_reference_chars: usize,
}

fn default_max_concurrent() -> usize { 64 }
fn default_backend_timeout_ms() -> u64 { 120_000 }
fn default_panel_timeout_ms() -> u64 { 90_000 }
fn default_router_policy() -> String { "always_fuse".into() }
fn default_samples() -> usize { 3 }
fn default_true() -> bool { true }
fn default_layers() -> usize { 1 }
fn default_max_reference_chars() -> usize { 8_000 }

impl Config {
    /// Validate every profile: loop guard, quorum bounds, unique names, single layer.
    pub fn validate(&self) -> Result<(), Error> {
        let mut seen = std::collections::HashSet::new();
        for p in &self.profiles {
            if !seen.insert(p.name.as_str()) {
                return Err(Error::Config(format!("duplicate profile name: {}", p.name)));
            }
            for model in p.all_models() {
                if model.starts_with("fusion/") {
                    return Err(Error::Config(format!(
                        "profile {}: model {model} references a fusion alias (loop)",
                        p.name
                    )));
                }
            }
            let n = p.panel.members.len();
            if n == 0 {
                return Err(Error::Config(format!("profile {}: empty panel", p.name)));
            }
            if p.panel.min_quorum < 1 || p.panel.min_quorum > n {
                return Err(Error::Config(format!(
                    "profile {}: min_quorum {} out of range 1..={n}",
                    p.name, p.panel.min_quorum
                )));
            }
            if p.aggregator.layers != 1 {
                return Err(Error::Config(format!(
                    "profile {}: multi-layer is not implemented yet (layers must be 1)",
                    p.name
                )));
            }
        }
        Ok(())
    }

    pub fn profile(&self, name: &str) -> Option<&Profile> {
        self.profiles.iter().find(|p| p.name == name)
    }
}

impl Profile {
    /// Every model id referenced by this profile, for the loop guard.
    fn all_models(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.router.single_model.as_str())
            .chain(self.panel.members.iter().map(String::as_str))
            .chain(std::iter::once(self.aggregator.judge.as_str()))
            .chain(std::iter::once(self.aggregator.synthesizer.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(name: &str, members: Vec<&str>, quorum: usize) -> Profile {
        Profile {
            name: name.into(),
            router: RouterConfig { policy: "always_fuse".into(), single_model: "b/s".into() },
            panel: PanelConfig {
                members: members.into_iter().map(Into::into).collect(),
                self_moa: false,
                samples: 3,
                min_quorum: quorum,
                timeout_ms: 90_000,
            },
            aggregator: AggregatorConfig {
                judge: "b/j".into(),
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

    fn cfg(profiles: Vec<Profile>) -> Config {
        Config {
            server: ServerConfig { bind: "0.0.0.0:8080".into(), max_concurrent_requests: 64 },
            backend: BackendConfig {
                base_url: "http://b/v1".into(),
                api_key_env: "K".into(),
                timeout_ms: 120_000,
            },
            profiles,
        }
    }

    #[test]
    fn valid_config_passes() {
        let c = cfg(vec![profile("research", vec!["b/a", "b/b"], 2)]);
        assert!(c.validate().is_ok());
    }

    #[test]
    fn rejects_fusion_alias_in_panel() {
        let c = cfg(vec![profile("research", vec!["fusion/research", "b/b"], 2)]);
        assert!(matches!(c.validate(), Err(Error::Config(_))));
    }

    #[test]
    fn rejects_quorum_out_of_range() {
        let c = cfg(vec![profile("research", vec!["b/a", "b/b"], 3)]);
        assert!(matches!(c.validate(), Err(Error::Config(_))));
    }

    #[test]
    fn rejects_duplicate_names() {
        let c = cfg(vec![
            profile("research", vec!["b/a", "b/b"], 2),
            profile("research", vec!["b/a", "b/b"], 2),
        ]);
        assert!(matches!(c.validate(), Err(Error::Config(_))));
    }

    #[test]
    fn rejects_multi_layer() {
        let mut p = profile("research", vec!["b/a", "b/b"], 2);
        p.aggregator.layers = 2;
        assert!(matches!(cfg(vec![p]).validate(), Err(Error::Config(_))));
    }
}
```

- [ ] **Step 2: Declare the module and run the tests**

Add to `chorus-core/src/lib.rs`:

```rust
pub mod config;
pub use config::{Config, Profile};
```

Run: `cargo test -p chorus-core config::tests`
Expected: PASS (5 tests).

- [ ] **Step 3: Commit**

```bash
git add chorus-core/src/config.rs chorus-core/src/lib.rs
git commit -s -m "feat: typed config with loop guard and quorum validation"
```

---

## Task 6: Router trait and AlwaysFuse (part of issue #8)

**Files:**
- Create: `chorus-core/src/router.rs`
- Modify: `chorus-core/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Create `chorus-core/src/router.rs`:

```rust
//! The router gate: decide whether to fuse or forward to a single model.

use async_trait::async_trait;

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
```

- [ ] **Step 2: Declare and run**

Add to `chorus-core/src/lib.rs`:

```rust
pub mod router;
pub use router::{RouteDecision, Router};
```

Run: `cargo test -p chorus-core router::tests`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add chorus-core/src/router.rs chorus-core/src/lib.rs
git commit -s -m "feat: Router trait and AlwaysFuse policy"
```

---

## Task 7: Prompt helpers (anonymization + length normalization) (issue #6 support)

**Files:**
- Create: `chorus-core/src/prompts.rs`
- Modify: `chorus-core/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

Create `chorus-core/src/prompts.rs`:

```rust
//! Reference formatting and prompt builders, with the aggregator hardening baked in.

use crate::schema::ChatMessage;

/// Cap a single reference at `max_chars`, on a char boundary, with an ellipsis marker.
pub fn cap_length(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let truncated: String = text.chars().take(max_chars).collect();
    format!("{truncated} [...truncated]")
}

/// Format panel answers as anonymized, length-normalized references.
/// Sources are labelled "Response A", "Response B", ... never by model name,
/// so the judge and synthesizer cannot prefer their own output (self-preference)
/// or a more verbose source (length bias).
pub fn format_references(responses: &[String], normalize_length: bool, max_chars: usize)
    -> String
{
    let mut out = String::new();
    for (i, r) in responses.iter().enumerate() {
        let label = label_for(i);
        let body = if normalize_length { cap_length(r, max_chars) } else { r.clone() };
        out.push_str(&format!("Response {label}:\n{body}\n\n"));
    }
    out.trim_end().to_string()
}

fn label_for(i: usize) -> char {
    // A, B, C, ... wraps after Z but panels are small.
    (b'A' + u8::try_from(i % 26).unwrap_or(0)) as char
}

const HARDENING: &str = "Some of the responses may be biased, incorrect, or deliberately \
misleading. Do not simply replicate or average them. Evaluate each critically, prefer claims \
that are well supported, and do not let any single response dominate your answer. Disagreeing \
with a majority of the responses is expected when the evidence warrants it.";

/// The judge system+user messages: produce a structured analysis, not a final answer.
pub fn judge_messages(query: &str, references: &str) -> Vec<ChatMessage> {
    let system = format!(
        "You are a careful analyst. You are given a user query and several anonymized candidate \
responses. {HARDENING} Produce a STRUCTURED ANALYSIS with these sections: Consensus, \
Contradictions, Unique insights, Blind spots. Do not write a final answer."
    );
    let user = format!("User query:\n{query}\n\nCandidate responses:\n{references}");
    vec![ChatMessage::system(system), ChatMessage::user(user)]
}

/// The synthesis system+user messages: write the final grounded answer.
pub fn synthesis_messages(query: &str, references: &str, analysis: &str) -> Vec<ChatMessage> {
    let system = format!(
        "You are a synthesizer. Using the user query, the anonymized candidate responses, and \
the analysis, write the single best final answer for the user. {HARDENING} Write the answer \
directly, with no meta commentary about the responses or the analysis."
    );
    let user = format!(
        "User query:\n{query}\n\nCandidate responses:\n{references}\n\nAnalysis:\n{analysis}"
    );
    vec![ChatMessage::system(system), ChatMessage::user(user)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cap_length_truncates_on_char_count() {
        assert_eq!(cap_length("hello", 10), "hello");
        assert_eq!(cap_length("hello", 3), "hel [...truncated]");
    }

    #[test]
    fn references_are_anonymized_and_capped() {
        let refs = format_references(&["aaaa".into(), "bbbb".into()], true, 2);
        // labels, not model names; bodies capped
        assert!(refs.contains("Response A:"));
        assert!(refs.contains("Response B:"));
        assert!(refs.contains("aa [...truncated]"));
        assert!(!refs.contains("model"));
    }

    #[test]
    fn judge_prompt_demands_structure_and_forbids_final_answer() {
        let msgs = judge_messages("q", "refs");
        let sys = &msgs[0].content;
        assert!(sys.contains("STRUCTURED ANALYSIS"));
        assert!(sys.contains("Do not write a final answer"));
        assert!(sys.contains("do not let any single response dominate"));
    }

    #[test]
    fn synthesis_prompt_carries_hardening() {
        let msgs = synthesis_messages("q", "refs", "analysis");
        assert!(msgs[0].content.contains("do not let any single response dominate"));
    }
}
```

- [ ] **Step 2: Declare and run**

Add to `chorus-core/src/lib.rs`:

```rust
pub mod prompts;
```

Run: `cargo test -p chorus-core prompts::tests`
Expected: PASS (4 tests).

- [ ] **Step 3: Commit**

```bash
git add chorus-core/src/prompts.rs chorus-core/src/lib.rs
git commit -s -m "feat: anonymized, length-normalized, hardened prompt builders"
```

---

## Task 8: Usage aggregation helper (issue #11 support)

**Files:**
- Create: `chorus-core/src/usage.rs`
- Modify: `chorus-core/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Create `chorus-core/src/usage.rs`:

```rust
//! Aggregate token usage across pipeline stages.

use crate::schema::Usage;

#[derive(Debug, Default, Clone)]
pub struct UsageAccumulator {
    total: Usage,
}

impl UsageAccumulator {
    pub fn add(&mut self, u: Option<&Usage>) {
        if let Some(u) = u {
            self.total.prompt_tokens += u.prompt_tokens;
            self.total.completion_tokens += u.completion_tokens;
            self.total.total_tokens += u.total_tokens;
        }
    }

    pub fn into_usage(self) -> Usage {
        self.total
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sums_present_skips_absent() {
        let mut acc = UsageAccumulator::default();
        acc.add(Some(&Usage { prompt_tokens: 1, completion_tokens: 2, total_tokens: 3 }));
        acc.add(None);
        acc.add(Some(&Usage { prompt_tokens: 4, completion_tokens: 5, total_tokens: 9 }));
        let u = acc.into_usage();
        assert_eq!(u, Usage { prompt_tokens: 5, completion_tokens: 7, total_tokens: 12 });
    }
}
```

- [ ] **Step 2: Declare and run**

Add to `chorus-core/src/lib.rs`:

```rust
pub mod usage;
pub use usage::UsageAccumulator;
```

Run: `cargo test -p chorus-core usage::tests`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add chorus-core/src/usage.rs chorus-core/src/lib.rs
git commit -s -m "feat: usage aggregation helper"
```

---

## Task 9: Panel fan-out with quorum and self_moa (issue #5)

**Files:**
- Create: `chorus-core/src/panel.rs`
- Modify: `chorus-core/src/lib.rs`

- [ ] **Step 1: Write the failing tests (with an in-memory mock backend)**

Create `chorus-core/src/panel.rs`:

```rust
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

pub struct PanelOutcome {
    /// Survivor answer contents, anonymized by position downstream.
    pub responses: Vec<String>,
    pub usage: Usage,
}

/// Build the per-call request for member `idx`.
fn member_request(base: &ChatCompletionRequest, model: &str, self_moa: bool, idx: usize)
    -> ChatCompletionRequest
{
    let mut req = base.clone();
    req.model = model.to_string();
    req.stream = false;
    if self_moa {
        // Vary temperature across samples of the single model for diversity.
        req.temperature = Some(0.3 + 0.2 * (idx as f32));
    }
    req
}

/// The list of (model, idx) calls this panel will make.
fn member_calls(cfg: &PanelConfig) -> Vec<(String, usize)> {
    if cfg.self_moa {
        let model = cfg.members.first().cloned().unwrap_or_default();
        (0..cfg.samples).map(|i| (model.clone(), i)).collect()
    } else {
        cfg.members.iter().cloned().enumerate().map(|(i, m)| (m, i)).collect()
    }
}

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
        return Err(Error::Quorum { got: responses.len(), needed: cfg.min_quorum });
    }

    Ok(PanelOutcome { responses, usage: acc.into_usage() })
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
                message: ChatMessage { role: "assistant".into(), content: content.into() },
                finish_reason: Some("stop".into()),
            }],
            usage: Some(Usage { prompt_tokens: 1, completion_tokens: 1, total_tokens: 2 }),
        }
    }

    #[async_trait]
    impl ChatBackend for MockBackend {
        async fn complete(&self, req: &ChatCompletionRequest)
            -> Result<ChatCompletionResponse, Error>
        {
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
        let backend = Arc::new(MockBackend { fail_first: 0, seen: AtomicUsize::new(0) });
        let out = run_panel(backend, &base_req(), &cfg(&["a", "b", "c"], 2, false))
            .await
            .unwrap();
        assert_eq!(out.responses.len(), 3);
        assert_eq!(out.usage.total_tokens, 6);
    }

    #[tokio::test]
    async fn proceeds_at_quorum_with_partial_failure() {
        let backend = Arc::new(MockBackend { fail_first: 1, seen: AtomicUsize::new(0) });
        let out = run_panel(backend, &base_req(), &cfg(&["a", "b", "c"], 2, false))
            .await
            .unwrap();
        assert_eq!(out.responses.len(), 2);
    }

    #[tokio::test]
    async fn fails_below_quorum() {
        let backend = Arc::new(MockBackend { fail_first: 2, seen: AtomicUsize::new(0) });
        let err = run_panel(backend, &base_req(), &cfg(&["a", "b", "c"], 2, false))
            .await
            .unwrap_err();
        assert!(matches!(err, Error::Quorum { got: 1, needed: 2 }));
    }

    #[tokio::test]
    async fn self_moa_samples_one_model() {
        let backend = Arc::new(MockBackend { fail_first: 0, seen: AtomicUsize::new(0) });
        let out = run_panel(backend, &base_req(), &cfg(&["only"], 1, true))
            .await
            .unwrap();
        assert_eq!(out.responses.len(), 3); // samples, not members
        assert!(out.responses.iter().all(|r| r == "answer-from-only"));
    }
}
```

- [ ] **Step 2: Declare and run**

Add to `chorus-core/src/lib.rs`:

```rust
pub mod panel;
pub use panel::{run_panel, PanelOutcome};
```

Run: `cargo test -p chorus-core panel::tests`
Expected: PASS (4 tests).

- [ ] **Step 3: Commit**

```bash
git add chorus-core/src/panel.rs chorus-core/src/lib.rs
git commit -s -m "feat: panel fan-out with quorum and self-MoA"
```

---

## Task 10: Judge and synthesis calls (issues #6, #7)

**Files:**
- Create: `chorus-core/src/judge.rs`, `chorus-core/src/synthesis.rs`
- Modify: `chorus-core/src/lib.rs`

- [ ] **Step 1: Write the failing tests for judge**

Create `chorus-core/src/judge.rs`:

```rust
//! The judge: a structured analysis over anonymized panel answers.

use std::sync::Arc;

use crate::backend::ChatBackend;
use crate::config::AggregatorConfig;
use crate::error::Error;
use crate::prompts::{format_references, judge_messages};
use crate::schema::{ChatCompletionRequest, Usage};

pub struct JudgeOutcome {
    pub analysis: String,
    pub usage: Option<Usage>,
}

pub async fn run_judge(
    backend: Arc<dyn ChatBackend>,
    cfg: &AggregatorConfig,
    query: &str,
    responses: &[String],
) -> Result<JudgeOutcome, Error> {
    let references = format_references(responses, cfg.normalize_length, cfg.max_reference_chars);
    let req = ChatCompletionRequest {
        model: cfg.judge.clone(),
        messages: judge_messages(query, &references),
        stream: false,
        temperature: Some(0.0),
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
        async fn complete(&self, req: &ChatCompletionRequest)
            -> Result<ChatCompletionResponse, Error>
        {
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
                usage: Some(Usage { prompt_tokens: 2, completion_tokens: 2, total_tokens: 4 }),
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
        let backend = Arc::new(Capturing { last_model: Mutex::new(String::new()) });
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
```

- [ ] **Step 2: Write synthesis**

Create `chorus-core/src/synthesis.rs`:

```rust
//! The synthesizer: the final grounded answer under the hardened prompt.

use std::sync::Arc;

use crate::backend::ChatBackend;
use crate::config::AggregatorConfig;
use crate::error::Error;
use crate::prompts::{format_references, synthesis_messages};
use crate::schema::{ChatCompletionRequest, ChatCompletionResponse};

/// Build the synthesis request (also used by the streaming path in the server).
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
        messages: synthesis_messages(query, &references, analysis),
        stream,
        temperature: Some(0.3),
    }
}

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
```

- [ ] **Step 3: Declare modules and run**

Add to `chorus-core/src/lib.rs`:

```rust
pub mod judge;
pub mod synthesis;
pub use judge::{run_judge, JudgeOutcome};
pub use synthesis::{run_synthesis, synthesis_request};
```

Run: `cargo test -p chorus-core judge::tests synthesis::tests`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add chorus-core/src/judge.rs chorus-core/src/synthesis.rs chorus-core/src/lib.rs
git commit -s -m "feat: judge and synthesis calls"
```

---

## Task 11: Pipeline orchestration (issue #8)

**Files:**
- Create: `chorus-core/src/pipeline.rs`
- Modify: `chorus-core/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

Create `chorus-core/src/pipeline.rs`:

```rust
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

pub struct Pipeline {
    backend: Arc<dyn ChatBackend>,
    router: Arc<dyn Router>,
}

impl Pipeline {
    /// Construct with the M1 `AlwaysFuse` router.
    pub fn new(backend: Arc<dyn ChatBackend>) -> Self {
        Self { backend, router: Arc::new(AlwaysFuse) }
    }

    pub async fn run(&self, profile: &Profile, req: &ChatCompletionRequest)
        -> Result<ChatCompletionResponse, Error>
    {
        // Router gate. Fail-open to fusion is implicit: AlwaysFuse never errors.
        if self.router.decide(req).await == RouteDecision::Single {
            let mut single = req.clone();
            single.model = profile.router.single_model.clone();
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
                message: ChatMessage { role: "assistant".into(), content: content.into() },
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
        async fn complete(&self, req: &ChatCompletionRequest)
            -> Result<ChatCompletionResponse, Error>
        {
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
            router: RouterConfig { policy: "always_fuse".into(), single_model: "b/single".into() },
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
}
```

- [ ] **Step 2: Declare and run**

Add to `chorus-core/src/lib.rs`:

```rust
pub mod pipeline;
pub use pipeline::Pipeline;
```

Run: `cargo test -p chorus-core pipeline::tests`
Expected: PASS.

- [ ] **Step 3: Add the judge-degradation test**

Append to `pipeline.rs` `tests`:

```rust
    struct JudgeFailsBackend;

    #[async_trait]
    impl ChatBackend for JudgeFailsBackend {
        async fn complete(&self, req: &ChatCompletionRequest)
            -> Result<ChatCompletionResponse, Error>
        {
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
```

Run: `cargo test -p chorus-core pipeline::tests`
Expected: PASS (2 tests).

- [ ] **Step 4: Commit**

```bash
git add chorus-core/src/pipeline.rs chorus-core/src/lib.rs
git commit -s -m "feat: pipeline orchestration with judge degradation"
```

---

## Task 12: Server state, error mapping, app, handlers (issue #9)

**Files:**
- Create: `chorus-server/src/state.rs`, `chorus-server/src/error.rs`, `chorus-server/src/app.rs`, `chorus-server/src/handlers.rs`, `chorus-server/src/metrics.rs`
- Modify: `chorus-server/src/main.rs`

- [ ] **Step 1: Write the metrics installer**

Create `chorus-server/src/metrics.rs`:

```rust
//! Prometheus recorder install and handle for the /metrics endpoint.

use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};

pub fn install() -> PrometheusHandle {
    PrometheusBuilder::new()
        .install_recorder()
        .expect("install prometheus recorder")
}
```

- [ ] **Step 2: Write the error mapping**

Create `chorus-server/src/error.rs`:

```rust
//! Map a core Error to an OpenAI-shaped error response.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chorus_core::Error;
use serde_json::json;

pub struct ApiError(pub Error);

impl From<Error> for ApiError {
    fn from(e: Error) -> Self {
        ApiError(e)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, kind) = match &self.0 {
            Error::UnknownProfile(_) | Error::InvalidModel(_) => {
                (StatusCode::NOT_FOUND, "invalid_request_error")
            }
            Error::Config(_) => (StatusCode::BAD_REQUEST, "invalid_request_error"),
            Error::Quorum { .. } | Error::Backend(_) | Error::Synthesis(_) => {
                (StatusCode::BAD_GATEWAY, "upstream_error")
            }
            Error::Timeout => (StatusCode::GATEWAY_TIMEOUT, "timeout"),
        };
        let body = json!({
            "error": { "message": self.0.to_string(), "type": kind }
        });
        (status, Json(body)).into_response()
    }
}
```

- [ ] **Step 3: Write the app state**

Create `chorus-server/src/state.rs`:

```rust
//! Shared application state.

use std::sync::Arc;

use chorus_core::config::Config;
use chorus_core::Pipeline;
use metrics_exporter_prometheus::PrometheusHandle;
use tokio::sync::Semaphore;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub pipeline: Arc<Pipeline>,
    pub limiter: Arc<Semaphore>,
    pub metrics: PrometheusHandle,
}
```

- [ ] **Step 4: Write the handlers (non-streaming first)**

Create `chorus-server/src/handlers.rs`:

```rust
//! HTTP handlers for the OpenAI surface.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use chorus_core::schema::ChatCompletionRequest;
use chorus_core::Error;
use serde_json::json;

use crate::error::ApiError;
use crate::state::AppState;

pub async fn healthz() -> StatusCode {
    StatusCode::OK
}

pub async fn metrics(State(state): State<AppState>) -> String {
    state.metrics.render()
}

pub async fn models(State(state): State<AppState>) -> Json<serde_json::Value> {
    let data: Vec<_> = state
        .config
        .profiles
        .iter()
        .map(|p| json!({ "id": format!("fusion/{}", p.name), "object": "model" }))
        .collect();
    Json(json!({ "object": "list", "data": data }))
}

/// Resolve the profile name from a `fusion/<name>` model alias.
fn profile_name(model: &str) -> Result<&str, Error> {
    model
        .strip_prefix("fusion/")
        .ok_or_else(|| Error::InvalidModel(model.to_string()))
}

pub async fn chat_completions(
    State(state): State<AppState>,
    Json(req): Json<ChatCompletionRequest>,
) -> Result<Response, ApiError> {
    let name = profile_name(&req.model)?;
    let profile = state
        .config
        .profile(name)
        .ok_or_else(|| Error::UnknownProfile(name.to_string()))?
        .clone();

    // Bound the fan-out amplification: one request holds one permit for its lifetime.
    let _permit = state
        .limiter
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| Error::Backend("limiter closed".into()))?;

    metrics::counter!("chorus_requests_total", "profile" => name.to_string()).increment(1);

    if req.stream {
        return Ok(crate::sse::stream_fusion(state.clone(), profile, req).await);
    }

    let resp = state.pipeline.run(&profile, &req).await?;
    Ok((StatusCode::OK, Json(resp)).into_response())
}
```

- [ ] **Step 5: Write a placeholder sse module so it compiles**

Create `chorus-server/src/sse.rs`:

```rust
//! Streaming synthesis. Filled in by Task 13.

use axum::response::{IntoResponse, Response};
use chorus_core::config::Profile;
use chorus_core::schema::ChatCompletionRequest;

use crate::state::AppState;

pub async fn stream_fusion(
    _state: AppState,
    _profile: Profile,
    _req: ChatCompletionRequest,
) -> Response {
    // Replaced in Task 13.
    axum::http::StatusCode::NOT_IMPLEMENTED.into_response()
}
```

- [ ] **Step 6: Write the app builder**

Create `chorus-server/src/app.rs`:

```rust
//! Build the axum application.

use axum::routing::{get, post};
use axum::Router;
use tower_http::trace::TraceLayer;

use crate::handlers;
use crate::state::AppState;

pub fn build(state: AppState) -> Router {
    Router::new()
        .route("/v1/chat/completions", post(handlers::chat_completions))
        .route("/v1/models", get(handlers::models))
        .route("/healthz", get(handlers::healthz))
        .route("/metrics", get(handlers::metrics))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
```

- [ ] **Step 7: Wire modules in main and add an integration test**

Replace `chorus-server/src/main.rs`:

```rust
mod app;
mod error;
mod handlers;
mod metrics;
mod sse;
mod state;

use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use chorus_core::backend::OpenAiBackend;
use chorus_core::config::Config;
use chorus_core::Pipeline;
use figment::providers::{Env, Format, Toml};
use figment::Figment;
use tokio::net::TcpListener;
use tokio::sync::Semaphore;

use crate::state::AppState;

fn load_config() -> anyhow::Result<Config> {
    let path = std::env::var("CHORUS_CONFIG").unwrap_or_else(|_| "config.toml".into());
    let config: Config = Figment::new()
        .merge(Toml::file(path))
        .merge(Env::prefixed("CHORUS_").split("__"))
        .extract()
        .context("load config")?;
    config.validate().context("validate config")?;
    Ok(config)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .json()
        .init();

    let config = load_config()?;
    let api_key = std::env::var(&config.backend.api_key_env)
        .with_context(|| format!("backend api key env {}", config.backend.api_key_env))?;

    let backend = Arc::new(OpenAiBackend::new(
        config.backend.base_url.clone(),
        api_key,
        Duration::from_millis(config.backend.timeout_ms),
    )?);

    let state = AppState {
        limiter: Arc::new(Semaphore::new(config.server.max_concurrent_requests)),
        pipeline: Arc::new(Pipeline::new(backend)),
        metrics: metrics::install(),
        config: Arc::new(config),
    };

    let bind = state.config.server.bind.clone();
    let listener = TcpListener::bind(&bind).await.with_context(|| format!("bind {bind}"))?;
    tracing::info!(%bind, "chorus listening");

    axum::serve(listener, app::build(state))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("serve")?;
    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("shutting down");
}
```

- [ ] **Step 8: Run server build and clippy**

Run: `cargo build -p chorus-server && cargo clippy -p chorus-server --all-targets -- -D warnings`
Expected: builds clean.

- [ ] **Step 9: Commit**

```bash
git add chorus-server/src/
git commit -s -m "feat: chorus-server app, handlers, state, error mapping, metrics"
```

---

## Task 13: Streaming synthesis over SSE (issue #10)

**Files:**
- Modify: `chorus-server/src/sse.rs`

- [ ] **Step 1: Replace the sse module with the real implementation**

Replace `chorus-server/src/sse.rs`:

```rust
//! Streaming: run the pipeline buffered, then stream the synthesized answer.
//!
//! The router, panel, and judge run buffered (they cannot stream). While they
//! run, the SSE stream emits keepalive comments so clients do not time out.
//! The final answer is then emitted as OpenAI chat.completion.chunk events.

use std::convert::Infallible;
use std::time::Duration;

use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use chorus_core::config::Profile;
use chorus_core::schema::ChatCompletionRequest;
use futures::Stream;
use serde_json::json;
use tokio::time::interval;

use crate::state::AppState;

pub async fn stream_fusion(
    state: AppState,
    profile: Profile,
    req: ChatCompletionRequest,
) -> Response {
    let model = format!("fusion/{}", profile.name);
    let stream = async_stream::stream! {
        // Run the whole pipeline buffered, racing a keepalive ticker.
        let mut ticker = interval(Duration::from_secs(5));
        ticker.tick().await; // first tick is immediate; skip it
        let pipeline = state.pipeline.clone();
        let fut = pipeline.run(&profile, &req);
        tokio::pin!(fut);

        let result = loop {
            tokio::select! {
                r = &mut fut => break r,
                _ = ticker.tick() => {
                    yield Ok::<Event, Infallible>(Event::default().comment("keepalive"));
                }
            }
        };

        match result {
            Ok(resp) => {
                let chunk = json!({
                    "id": resp.id,
                    "object": "chat.completion.chunk",
                    "created": resp.created,
                    "model": model,
                    "choices": [{
                        "index": 0,
                        "delta": { "role": "assistant", "content": resp.first_content() },
                        "finish_reason": "stop"
                    }]
                });
                yield Ok(Event::default().data(chunk.to_string()));
                yield Ok(Event::default().data("[DONE]"));
            }
            Err(e) => {
                let err = json!({ "error": { "message": e.to_string(), "type": "upstream_error" } });
                yield Ok(Event::default().data(err.to_string()));
                yield Ok(Event::default().data("[DONE]"));
            }
        }
    };

    sse_response(stream)
}

fn sse_response<S>(stream: S) -> Response
where
    S: Stream<Item = Result<Event, Infallible>> + Send + 'static,
{
    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}
```

- [ ] **Step 2: Build and clippy**

Run: `cargo build -p chorus-server && cargo clippy -p chorus-server --all-targets -- -D warnings`
Expected: builds clean.

- [ ] **Step 3: Commit**

```bash
git add chorus-server/src/sse.rs
git commit -s -m "feat: streaming synthesis over SSE with keepalive"
```

---

## Task 14: Server integration tests against wiremock (issue #12)

**Files:**
- Create: `chorus-server/tests/fusion.rs`

- [ ] **Step 1: Make the app testable from an integration test**

The app is built from `AppState`, which needs a `Config` and a `Pipeline`. Add a small test-only constructor path by exposing `app::build` and `state::AppState` (already `pub`). The integration test builds a `Config` pointing `base_url` at a wiremock server, builds the real `OpenAiBackend`, and drives the axum app in-process with `tower::ServiceExt::oneshot`.

Add to `chorus-server/Cargo.toml` `[dev-dependencies]`:

```toml
tower = { version = "0.5", features = ["util"] }
http-body-util = "0.1"
```

- [ ] **Step 2: Write the integration test**

Create `chorus-server/tests/fusion.rs`:

```rust
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use chorus_core::backend::OpenAiBackend;
use chorus_core::config::{
    AggregatorConfig, BackendConfig, Config, PanelConfig, Profile, RouterConfig, ServerConfig,
};
use chorus_core::Pipeline;
use http_body_util::BodyExt;
use metrics_exporter_prometheus::PrometheusBuilder;
use tokio::sync::Semaphore;
use tower::ServiceExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// Re-declare the server modules under test via the binary's path is not possible,
// so the test exercises the public crate API. The app builder and state live in
// the binary crate; expose them by adding `pub mod` aliases in a `lib.rs` (see note).

fn body_for(content: &str) -> serde_json::Value {
    serde_json::json!({
        "id": "x", "object": "chat.completion", "created": 1, "model": "m",
        "choices": [{"index": 0, "message": {"role": "assistant", "content": content},
            "finish_reason": "stop"}],
        "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
    })
}

fn test_config(base_url: String) -> Config {
    Config {
        server: ServerConfig { bind: "127.0.0.1:0".into(), max_concurrent_requests: 8 },
        backend: BackendConfig { base_url, api_key_env: "UNUSED".into(), timeout_ms: 5_000 },
        profiles: vec![Profile {
            name: "research".into(),
            router: RouterConfig { policy: "always_fuse".into(), single_model: "b/s".into() },
            panel: PanelConfig {
                members: vec!["b/a".into(), "b/b".into()],
                self_moa: false,
                samples: 3,
                min_quorum: 1,
                timeout_ms: 5_000,
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
        }],
    }
}

#[tokio::test]
async fn end_to_end_fused_completion() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body_for("FUSED")))
        .mount(&upstream)
        .await;

    let config = test_config(format!("{}/v1", upstream.uri()));
    let backend = Arc::new(
        OpenAiBackend::new(config.backend.base_url.clone(), "k", Duration::from_secs(5)).unwrap(),
    );
    let state = chorus_server::state::AppState {
        limiter: Arc::new(Semaphore::new(8)),
        pipeline: Arc::new(Pipeline::new(backend)),
        metrics: PrometheusBuilder::new().build_recorder().handle(),
        config: Arc::new(config),
    };
    let app = chorus_server::app::build(state);

    let req = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"model":"fusion/research","messages":[{"role":"user","content":"q"}]}"#,
        ))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["choices"][0]["message"]["content"], "FUSED");
    assert_eq!(v["model"], "fusion/research");
}
```

- [ ] **Step 3: Expose the server modules to integration tests**

Integration tests can only see a crate's library target, so add a thin library alongside the binary. Create `chorus-server/src/lib.rs`:

```rust
//! Library facade so integration tests can build the app.
pub mod app;
pub mod error;
pub mod handlers;
pub mod metrics;
pub mod sse;
pub mod state;
```

Update `chorus-server/Cargo.toml` to declare both targets:

```toml
[lib]
name = "chorus_server"
path = "src/lib.rs"

[[bin]]
name = "chorus-server"
path = "src/main.rs"
```

And change `chorus-server/src/main.rs` to use the library instead of `mod` decls: replace the six `mod ...;` lines with:

```rust
use chorus_server::{app, metrics, state};
```

(Keep the rest of `main.rs` unchanged; it already references `app::build`, `metrics::install`, `state::AppState`.)

- [ ] **Step 4: Run the integration test**

Run: `cargo test -p chorus-server --test fusion`
Expected: PASS.

- [ ] **Step 5: Run the whole suite**

Run: `cargo test --workspace`
Expected: all green, no network used.

- [ ] **Step 6: Commit**

```bash
git add chorus-server/Cargo.toml chorus-server/src/lib.rs chorus-server/src/main.rs chorus-server/tests/fusion.rs
git commit -s -m "test: end-to-end fused completion against a wiremock backend"
```

---

## Task 15: Example config and metrics emission in the pipeline path (issue #11)

**Files:**
- Create: `config.example.toml`
- Modify: `chorus-server/src/handlers.rs` (stage timing metric)

- [ ] **Step 1: Write the example config**

Create `config.example.toml`:

```toml
[server]
bind = "0.0.0.0:8080"
max_concurrent_requests = 64

[backend]
base_url = "http://localhost:8000/v1"
api_key_env = "CHORUS_BACKEND_KEY"
timeout_ms = 120000

[[profiles]]
name = "research"

  [profiles.router]
  policy = "always_fuse"
  single_model = "your/strong-model"

  [profiles.panel]
  members = ["your/model-a", "your/model-b", "your/model-c"]
  self_moa = false
  min_quorum = 2
  timeout_ms = 90000

  [profiles.aggregator]
  judge = "your/strong-model"
  synthesizer = "your/other-strong-model"
  anonymize_sources = true
  normalize_length = true
  single_source_cap = true
  layers = 1
```

- [ ] **Step 2: Add a request-latency histogram around the pipeline call**

In `chorus-server/src/handlers.rs`, replace the non-streaming branch at the end of `chat_completions`:

```rust
    let started = std::time::Instant::now();
    let resp = state.pipeline.run(&profile, &req).await?;
    metrics::histogram!("chorus_request_seconds", "profile" => name.to_string())
        .record(started.elapsed().as_secs_f64());
    Ok((StatusCode::OK, Json(resp)).into_response())
```

- [ ] **Step 3: Verify metrics render**

Add to `chorus-server/tests/fusion.rs`:

```rust
#[tokio::test]
async fn metrics_endpoint_exposes_request_counter() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body_for("FUSED")))
        .mount(&upstream)
        .await;
    let config = test_config(format!("{}/v1", upstream.uri()));
    let backend = Arc::new(
        OpenAiBackend::new(config.backend.base_url.clone(), "k", Duration::from_secs(5)).unwrap(),
    );
    let recorder = PrometheusBuilder::new().build_recorder();
    let handle = recorder.handle();
    metrics::set_global_recorder(recorder).ok();
    let state = chorus_server::state::AppState {
        limiter: Arc::new(Semaphore::new(8)),
        pipeline: Arc::new(Pipeline::new(backend)),
        metrics: handle,
        config: Arc::new(config),
    };
    let app = chorus_server::app::build(state);

    let post = Request::builder()
        .method("POST").uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"model":"fusion/research","messages":[{"role":"user","content":"q"}]}"#,
        )).unwrap();
    let _ = app.clone().oneshot(post).await.unwrap();

    let get = Request::builder().uri("/metrics").body(Body::empty()).unwrap();
    let resp = app.oneshot(get).await.unwrap();
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(text.contains("chorus_requests_total"));
}
```

Run: `cargo test -p chorus-server --test fusion`
Expected: PASS (note: `set_global_recorder` may already be set by another test; the `.ok()` tolerates that, and the counter still records via the handler's macro against the global recorder).

- [ ] **Step 4: Commit**

```bash
git add config.example.toml chorus-server/src/handlers.rs chorus-server/tests/fusion.rs
git commit -s -m "feat: example config and request-latency metric"
```

---

## Task 16: Packaging and CI (issue #13)

**Files:**
- Create: `Dockerfile`, `.dockerignore`, `.github/workflows/ci.yaml`

- [ ] **Step 1: Write the Dockerfile (cargo-chef, distroless)**

Create `Dockerfile`:

```dockerfile
FROM rust:1-slim AS chef
RUN cargo install cargo-chef --locked
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release -p chorus-server

FROM gcr.io/distroless/cc-debian12:nonroot
COPY --from=builder /app/target/release/chorus-server /usr/local/bin/chorus-server
USER nonroot
ENTRYPOINT ["/usr/local/bin/chorus-server"]
```

Create `.dockerignore`:

```
target
.git
```

- [ ] **Step 2: Write the CI workflow**

Create `.github/workflows/ci.yaml`:

```yaml
name: CI
on:
  push:
    branches: [main]
  pull_request:
permissions:
  contents: read
jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: clippy, rustfmt
      - uses: Swatinem/rust-cache@v2
      - run: cargo fmt --all -- --check
      - run: cargo clippy --workspace --all-targets -- -D warnings
      - run: cargo test --workspace
  deny:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: EmbarkStudios/cargo-deny-action@v2
  docker-build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: docker build -t chorus:ci .
```

- [ ] **Step 3: Verify locally**

Run: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: all green.
Run: `docker build -t chorus:ci .` (if docker is available)
Expected: image builds; `docker run --rm chorus:ci --help` or a missing-config error exits cleanly.

- [ ] **Step 4: Commit and open a PR**

```bash
git add Dockerfile .dockerignore .github/workflows/ci.yaml
git commit -s -m "ci: workspace test, lint, deny, and docker build"
git push -u origin feat/m1-fusion-core
gh pr create -R paperclipinc/chorus --fill
```

Note: codeql and scorecard workflows (named in CLAUDE.md) are added in a follow-up alongside branch protection; this task delivers the four core required checks (test, lint via the test job, deny, docker-build).

---

## Self-Review

**1. Spec coverage** (against `2026-06-18-chorus-fusion-gateway-design.md`, M1 scope):

- Router gate (AlwaysFuse + interface): Tasks 6, 11. The classifier policy is M2 (out of this plan), correctly deferred.
- Panel fan-out, quorum, self_moa: Task 9.
- Judge with anonymization + length normalization: Tasks 7, 10.
- Hardened synthesis (dissent, single-source cap, synthesizer not self-judging): Tasks 7, 10; the not-self-judging default is a config convention plus the anonymization in Task 7 (structural enforcement beyond the prompt is M3 issue #21, correctly deferred).
- Pipeline single-layer + judge degradation: Task 11.
- OpenAI server surface + concurrency limiter + graceful shutdown: Task 12.
- Streaming synthesis with keepalive: Task 13.
- Usage accounting + metrics: Tasks 8, 12, 15.
- wiremock integration tests: Tasks 4, 9, 14, 15.
- Packaging + CI: Task 16.
- Tools: interface is M2 per the spec; this plan ships `tools` as a config field only (Task 5), which matches "interface only, none wired."

**2. Placeholder scan:** No "TBD"/"add error handling"/"similar to Task N". The Task 12 `sse.rs` placeholder is explicitly replaced in Task 13 with full code, and the dependency between them is stated.

**3. Type consistency:** `ChatCompletionRequest`, `ChatCompletionResponse`, `Usage`, `ChatMessage`, `Choice` defined once in Task 3 and used unchanged. `PanelOutcome.responses: Vec<String>` produced in Task 9 is consumed by `run_judge`/`synthesis_request` (Task 10) and `Pipeline::run` (Task 11) as `&[String]`. `AggregatorConfig`, `PanelConfig`, `RouterConfig`, `Profile` defined in Task 5 and reused verbatim in later test fixtures. `Pipeline::new(Arc<dyn ChatBackend>)`, `Pipeline::run(&Profile, &ChatCompletionRequest)` consistent across Tasks 11, 12, 14, 15. `AppState` fields (`config`, `pipeline`, `limiter`, `metrics`) consistent across Tasks 11(state), 12, 14, 15.

One deviation handled: Task 14 introduces a `chorus-server` library target so integration tests can reach `app::build` and `AppState`; Task 14 Step 3 updates `main.rs` accordingly. This is called out explicitly rather than left implicit.
