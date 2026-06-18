# Roadmap

Ordered by priority. The rule that orders it: **no unverified claims, and correctness before
cost.** A gateway that reports a quality lift it cannot reproduce, or that fuses answers a
single model would have answered better, is worth nothing.

Status legend: ✅ done · 🔨 in progress · ⬜ not started

The design and the evidence behind each decision live in [`docs/`](docs/). GitHub issues map
this roadmap.

## M1: quality spike

Prove that fused answers beat a single model on hard tasks, with the cost recorded. Ships
`AlwaysFuse` routing (the routing CONFOUND is removed so the fusion question is isolated),
single-layer, and the hardened anonymized synthesis judge. No tools.

- ⬜ Cargo workspace: `chorus-core` library + `chorus-server` binary; `#![forbid(unsafe_code)]`,
  clippy pedantic, rustfmt, `rust-toolchain.toml`, `deny.toml`.
- ⬜ OpenAI chat-completion schema (`schema`): request, response, streaming chunk, error shapes.
- ⬜ Backend (`backend`): `trait ChatBackend` + `OpenAiBackend` (reqwest + rustls, pooled),
  surfacing upstream errors in OpenAI error shape.
- ⬜ Config (`config`): typed profiles (router, panel, aggregator), figment TOML + env, and
  validation: loop guard (no `fusion/*` in any model slot), quorum bounds, unique names.
- ⬜ Panel (`panel`): concurrent fan-out (`tokio::JoinSet`), per-call timeout, partial-failure
  quorum, and the `self_moa` sampling mode.
- ⬜ Judge (`judge`): structured-analysis schema (consensus, contradictions, unique insights,
  blind spots) with source anonymization and length normalization.
- ⬜ Synthesis (`synthesis`): hardened final-answer prompt (mandatory critical dissent,
  single-source dominance cap, synthesizer not in its own panel).
- ⬜ Pipeline (`pipeline`): router (`AlwaysFuse`) -> panel -> judge -> synthesis; single layer;
  fail-open router, graceful judge degradation.
- ⬜ Server (`chorus-server`): axum routes `/v1/chat/completions`, `/v1/models`, `/healthz`,
  `/metrics`; concurrency limiter; graceful shutdown; tracing.
- ⬜ Streaming: SSE for the synthesis step, with keepalive comments during the buffered phases.
- ⬜ Usage accounting: aggregate `usage` across all stages; per-model and per-stage token metrics.
- ⬜ Tests: unit (config validation, prompt builders via insta snapshots, usage aggregation) +
  `wiremock` integration (quorum, judge degradation, streaming, timeout cancellation).
- ⬜ Packaging: multi-stage Dockerfile (cargo-chef, distroless); CI (test, lint, deny, docker-build,
  codeql, scorecard); release-please.
- ⬜ Benchmark: target a fusion profile vs a single-model baseline; record quality, cost, latency.

## M2: the router gate and the sovereignty proof

Make it cost-optimal and produce the headline datapoint.

- ⬜ Router policy: a cheap difficulty classifier (`trait Router` impl) with a per-deployment
  threshold; fail-open to fusion.
- ⬜ Threshold calibration on real traffic; report the routed cost-quality curve (single vs
  fused-only vs routed).
- ⬜ Budget-panel curation and the "a budget panel fused beats a single frontier model"
  benchmark writeup.
- ⬜ Tool-provider interface + reference SearXNG adapter (open source, self-hosted), off by
  default, enabled only if web access is needed for comparability.

## M3: hardening and ecosystem

- ⬜ Multi-backend examples and docs (vLLM, Ollama, a hosted OpenAI-compatible API, a gateway).
- ⬜ Published, cosign-signed container image with an SBOM attestation.
- ⬜ Optional multi-layer (`layers > 1`) refine variant for offline or high-stakes paths,
  behind config, with the cost/latency curve documented.
- ⬜ Deception and outlier detection in the aggregator beyond the M1 prompt-level hardening.

## Out of scope

These are deliberately excluded; the design doc records the evidence.

- Token-level / logit fusion (needs shared vocab and same-host logits; incompatible with a
  multi-vendor OpenAI proxy).
- A pure choose-from-N voting aggregator (propagates majority errors).
- In-panel confidence-weighted aggregation (not established; confidence is used only in the
  router gate).
- Calling model providers directly (chorus always speaks to one OpenAI-compatible backend).
