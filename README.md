<h1 align="center">chorus</h1>

<p align="center">
  <b>An OpenAI-compatible Mixture-of-Agents fusion gateway.</b><br/>
  Fan one chat-completion request out to a panel of models, then synthesize one hardened answer. Self-hostable, backend-agnostic, exposed as a single model alias.
</p>

<p align="center">
  <a href="docs/">Documentation</a> .
  <a href="#what-is-chorus">What is chorus</a> .
  <a href="#how-it-works">How it works</a> .
  <a href="#comparison">Comparison</a> .
  <a href="ROADMAP.md">Roadmap</a> .
  <a href="CONTRIBUTING.md">Contributing</a>
</p>

---

> **Status: early.** The design is committed (see [`docs/`](docs/) and [ROADMAP.md](ROADMAP.md))
> and implementation is underway. This README describes the target design; every capability
> is tracked as an issue and gated on its milestone. Following the project's no-unverified-claims
> rule, no quality or cost numbers are stated here until they are reproducible from a committed
> benchmark.

## What is chorus

A single model is a single point of view. Mixture-of-Agents (MoA) asks several models the
same question in parallel and synthesizes their answers into one, which can beat any single
member on hard tasks. The published research supports this when it is done carefully, and it
also documents exactly how it goes wrong.

`chorus` is a self-hostable, OpenAI-compatible gateway that does MoA well:

- **OpenAI in, OpenAI out.** It speaks the chat-completions API and is registered as one
  model alias (`fusion/<profile>`), so any OpenAI-compatible client or agent uses it with
  zero integration work.
- **Backend-agnostic.** It talks to one OpenAI-compatible upstream and inherits that
  upstream's auth, routing, and logging. The same binary runs against a local vLLM or Ollama,
  a gateway like Bifrost, or a hosted API. It never talks to model providers directly.
- **Cost-aware by default.** A router gate decides whether a query is hard enough to fuse, so
  the expensive panel only fires when it earns its cost. Easy queries go straight to a single
  model.
- **Hardened against the known failure modes.** The aggregator is built to resist the social
  and herding biases that make naive MoA worse than a single model: source anonymization,
  length normalization, mandatory critical dissent, and a cap on any single source dominating.

It is developed open-core: this repository is Apache 2.0 in full, with no commercial feature
held back. See [docs/open-core.md](docs/open-core.md).

## How it works

```
client ──(model: fusion/research)──▶ chorus
                                        │
                                    router gate
                                    /          \
                              easy: 1 model   hard: panel fan-out
                                    \          /   judge (structured analysis)
                                     \        /    synthesis (one hardened answer)
                                      ▼      ▼
                                   OpenAI-compatible backend
```

1. **Router gate.** Score the query; forward easy ones to a single model, fuse the hard ones.
2. **Panel.** Fan out to a curated panel of comparable-strength models in parallel, tolerating
   partial failure down to a quorum. A self-MoA mode samples one strong model N times when one
   model clearly dominates.
3. **Judge.** A structured analysis over the anonymized, length-normalized answers: consensus,
   contradictions, unique insights, blind spots.
4. **Synthesis.** One model writes the final answer grounded in that analysis, under a prompt
   that mandates critical evaluation and forbids letting any single source dominate. Synthesis
   generates a new answer; it is not a vote.
5. **Usage.** Tokens spent across every stage are aggregated and reported, so the cost of a
   fused answer is never hidden.

The full design, including the evidence base behind each decision, is in
[docs/](docs/).

## Quickstart

> The interface below is the target shape and is tracked on the roadmap. Until the first
> release lands, build from source; see [CONTRIBUTING.md](CONTRIBUTING.md).

Define a profile in `config.toml`:

```toml
[backend]
base_url = "http://localhost:8000/v1"   # any OpenAI-compatible endpoint
api_key_env = "CHORUS_BACKEND_KEY"

[[profiles]]
name = "research"

  [profiles.router]
  policy = "always_fuse"
  single_model = "your/strong-model"

  [profiles.panel]
  members = ["your/model-a", "your/model-b", "your/model-c"]
  min_quorum = 2

  [profiles.aggregator]
  judge = "your/strong-model"
  synthesizer = "your/other-strong-model"
  anonymize_sources = true
```

Call it like any model:

```bash
curl http://localhost:8080/v1/chat/completions \
  -H "Authorization: Bearer $CHORUS_KEY" \
  -d '{"model": "fusion/research", "messages": [{"role": "user", "content": "..."}]}'
```

## Comparison

- **vs a single model.** chorus trades latency and tokens for quality on hard queries, and the
  router gate keeps that cost off easy queries. A single model is cheaper and faster when the
  question is easy, which is exactly when the router forwards to it.
- **vs OpenRouter Fusion.** Same panel/judge/synthesis idea, but self-hostable, backend-agnostic,
  and open source, so it runs entirely on your own infrastructure with no third-party data path.
- **vs the original Together MoA.** chorus is a maintained, production-shaped service (OpenAI
  surface, router gate, partial-failure quorum, streaming synthesis, usage accounting, aggregator
  hardening) rather than reference scripts, and it defaults to a single layer because deeper
  layers multiply cost and propagate a bad proposer's influence.
- **vs routing-only (e.g. RouteLLM).** Routing picks one model; chorus routes AND fuses. The
  router gate is the cost front door; fusion is what beats the single best model on the hard tail.

## Contributing

Contributions are welcome under the [DCO](CONTRIBUTING.md) (no CLA). The roadmap is the
priority order; start near the top. No em or en dashes anywhere; see the coding conventions
in [CLAUDE.md](CLAUDE.md).

## License

Apache 2.0. See [LICENSE](LICENSE). The "chorus" name and marks are reserved; see
[TRADEMARKS.md](TRADEMARKS.md).
