# CLAUDE.md

## Project Overview

`chorus` is a self-hostable, OpenAI-compatible Mixture-of-Agents (MoA) fusion gateway.
It fans one chat-completion request out to a panel of models in parallel, has a judge
model produce a structured analysis of their answers (consensus, contradictions, unique
insights, blind spots), then has a synthesizer model write one final answer grounded in
that analysis. The result is a single OpenAI-compatible response, returned under one model
alias (`fusion/<profile>`).

The core idea is not new: this is Mixture-of-Agents (Together AI, 2024). What chorus adds
is a clean, well-maintained, backend-agnostic implementation you can run yourself. The
backend is "any OpenAI-compatible endpoint," so the same binary runs unchanged against a
local vLLM or Ollama, against a gateway like Bifrost, or against a hosted OpenAI-compatible
API. chorus never talks to model providers directly; it speaks OpenAI to one upstream and
inherits that upstream's auth, routing, and logging.

Components:

- **chorus-core** (library): the engine. OpenAI request/response schema, the backend
  trait, the panel fan-out, the judge, the synthesizer, the pipeline, the tool-provider
  trait, and config with validation. Zero deployment coupling.
- **chorus-server** (binary): an axum HTTP service exposing `/v1/chat/completions`,
  `/v1/models`, `/healthz`, and `/metrics`. Loads config, wires tracing, applies a
  concurrency limiter, and serves the core.

ROADMAP.md is the priority order for all work. Issues map it.

## Operating Principles

These outrank convenience:

1. **No unverified claims.** Every public number (quality lift, cost, latency) must be
   reproducible from a committed benchmark or it does not get written. A README that
   describes behavior the code does not have is worth nothing.
2. **Backend-agnostic, no provider lock-in.** The core depends only on the OpenAI
   chat-completion contract. Anything provider-specific lives behind the backend trait or a
   tool adapter, never in the pipeline.
3. **Open source technologies only for integrations.** Tool adapters (web search and fetch)
   wire to open-source, self-hostable technology (the reference adapter is SearXNG), never
   to a proprietary SaaS.
4. **Boring failure behavior.** Every stage defines what happens on a slow or failed panel
   member, a failed judge, a backend outage, and capacity exhaustion. Partial failure
   degrades predictably; it does not crash a request.
5. **Honest cost.** MoA spends more tokens than a single call. The gateway accounts for
   every token it spends and surfaces it, so the cost of a fused answer is never hidden.

## Commands

```bash
cargo build --workspace            # build core + server
cargo test --workspace             # unit + integration tests
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check         # formatting gate
cargo deny check                   # license + advisory + ban audit
cargo run -p chorus-server         # run the gateway against config.toml
```

- Integration tests use a `wiremock` mock backend; no network or real models required.
- Run a single crate's tests: `cargo test -p chorus-core`.

## Architecture

- **schema** (`chorus-core/src/schema`): OpenAI chat-completion request/response types,
  hand-owned via serde for clarity and to avoid importing a client library's behavior.
- **backend** (`chorus-core/src/backend`): `trait ChatBackend` plus an `OpenAiBackend`
  built on reqwest with rustls. The only module that knows a URL exists.
- **panel** (`chorus-core/src/panel`): concurrent fan-out via `tokio::JoinSet`, per-call
  timeout, partial-failure tolerance down to a configurable quorum.
- **judge** (`chorus-core/src/judge`): the structured-analysis prompt and the parsed result
  schema.
- **synthesis** (`chorus-core/src/synthesis`): the final-answer synthesizer.
- **pipeline** (`chorus-core/src/pipeline`): orchestrates panel then judge then synthesis;
  honors the `layers` knob (default 1).
- **tools** (`chorus-core/src/tools`): `trait ToolProvider`; a reference SearXNG adapter
  behind a cargo feature, off by default.
- **config** (`chorus-core/src/config`): strongly-typed profiles plus validation.
- **server** (`chorus-server`): axum app, config load (figment), tracing, graceful
  shutdown, a `tokio::Semaphore` concurrency limiter.

Request flow for one `fusion/<profile>` call: parse, select profile, fan out to the panel,
keep survivors if at least quorum returned, run the judge (degrade to raw responses on judge
failure), run the synthesizer, aggregate token usage, return.

## Coding Conventions

### Punctuation (strict)

Never use em (U+2014) or en (U+2013) dashes anywhere: source, comments, doc comments,
Markdown, TOML, YAML, commit messages, PR descriptions, the GitHub repo description, and
release notes. Use only `.` `,` `;` `:` as connectors. ASCII hyphen-minus (-) is fine for
ranges and compound identifiers. If a tool inserts one (release-please, Dependabot), rewrite
it before merging.

### Rust style

- Edition 2024, latest stable toolchain (pinned in `rust-toolchain.toml`).
- `#![forbid(unsafe_code)]` in every crate.
- clippy with `-D warnings` and pedantic lints; rustfmt clean is a merge requirement.
- Errors: `thiserror` in `chorus-core`, `anyhow` only at the binary boundary. Errors
  returned to callers use the OpenAI error JSON shape.
- Async on tokio; HTTP via reqwest with rustls (no OpenSSL, for clean static builds).

### Secrets and data

Secret values (the backend API key, any tool credentials) are never logged, never in error
messages, never in span fields. Log keys and counts only. Prompt and completion content is
NOT logged by default; any opt-in content logging is gated behind explicit config and
documented. The backend treats the upstream key as opaque.

### Commits and branches

- Conventional commits: feat, fix, docs, ci, chore, refactor, test.
- Branch naming: feat/, fix/, chore/, docs/, ci/, refactor/.
- Sign off every commit (`git commit -s`); see CONTRIBUTING.md and the DCO check.

### TDD

Write the failing test first. Every behavior change lands with its test in the same commit.

### git

Stage explicit paths only; never `git add -A`. README claims follow the no-unverified-claims
rule: every number it states must be reproducible from a committed benchmark or carry an
explicit issue reference marking it as a target.

## CI Pipeline

Jobs (all required on main; main requires branches to be up to date):

- **test**: `cargo test --workspace` (unit + wiremock integration).
- **lint**: `cargo clippy --workspace --all-targets -- -D warnings` and `cargo fmt --check`.
- **deny**: `cargo deny check` (licenses, advisories, bans).
- **docker-build**: the `chorus-server` image, multi-stage with cargo-chef, distroless.
- **codeql** and **scorecard**: supply-chain and code scanning.

## Security Practices

- chorus proxies prompts and holds an upstream API key. The reportable surface is in
  SECURITY.md: key handling, prompt/response data handling, the fan-out amplification factor
  (one request spends N backend calls), and the backend/tool URL configuration.
- Security-sensitive paths get extra care and a named human reviewer before merge: the
  backend client, the config loop guard, and any tool adapter. Listed in `.github/CODEOWNERS`.
- Published images are cosign-signed (keyless OIDC) and carry an SBOM attestation; see
  SECURITY.md.

## Workflow Pointers

- ROADMAP.md is the priority order; GitHub issues map it.
- Plans live in `docs/plans/`.
- Every PR needs: tests in the same commit, docs updated in the same PR, and a benchmark run
  if a quality or cost claim changed.
