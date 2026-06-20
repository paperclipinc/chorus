# Design

This is the design of chorus, an OpenAI-compatible Mixture-of-Agents (MoA) fusion gateway.
It describes the open-source project only. A deployer's own infrastructure (which backend,
which models, how it is wired into a cluster) is configuration, not part of this design.

## Goal

A single model is a single point of view. MoA asks several models the same question in
parallel and synthesizes their answers into one, which can beat any single member on hard
tasks. The 2024-2026 literature supports this when it is done carefully, and documents
precisely how it fails. chorus implements the careful version behind a stable OpenAI surface.

## Principles

1. Backend-agnostic. The core depends only on the OpenAI chat-completion contract. chorus
   speaks OpenAI to one upstream and inherits that upstream's auth, routing, and logging. It
   never talks to model providers directly.
2. Correctness before cost. The aggregator is hardened against the documented MoA failure
   modes before any cost optimization is layered on.
3. No unverified claims. Every quality or cost number is reproducible from a committed
   benchmark or it is not stated.
4. Boring failure behavior. Every stage defines what happens on a slow or failed member, a
   failed judge, a backend outage, and capacity exhaustion.

## Architecture

chorus is a service that exposes the OpenAI chat-completions API and is registered as one
model alias (`fusion/<profile>`). A request flows through a pipeline:

```
request ─▶ router gate ─┬─ easy ─▶ single model ─▶ response
                        └─ hard ─▶ panel ─▶ judge ─▶ synthesis ─▶ response
```

Every model call goes to one configured OpenAI-compatible backend. A profile names the router
policy, the panel, and the aggregator. A config-load guard forbids any model slot from
referencing another `fusion/*` alias, so the gateway cannot recurse.

## Pipeline stages

### Router gate

A cheap pre-panel decision: is this query hard enough to justify the panel? Easy queries are
forwarded to a single strong model; hard queries are fused. This is the primary cost lever: a
system that always fuses is dominated on cost by one that routes.

The router is a trait with two implemented policies:

- `always_fuse`: every query goes through the full panel and synthesis path. The baseline.
- `classifier`: a cheap LLM-judge scores query difficulty on a 0.0 to 1.0 scale. Queries
  scoring below the configurable threshold go to a single strong model; those at or above
  the threshold go through the full fusion path. The classifier is implemented and ships
  with configurable `classifier_model` and `threshold` fields in the profile router config.
  It fails open to fusion on any backend error or unparseable score, so the cost saving is
  always conservative. The router decision is recorded in metrics so the saving is measured,
  not assumed.

Routers generalize across model pairs but degrade under domain shift. Threshold calibration
on real traffic is the remaining deploy-time step: the classifier is implemented; the
threshold value for a given workload must be tuned against observed traffic.

### Panel

Fan out to the panel concurrently, with a per-call timeout, tolerating partial failure down to
a configurable quorum. Proposer quality is primary and diversity is secondary: diversity among
comparable-strength models reduces correlated errors, but mixing in a weaker model can lower
average proposer quality enough to make the fused answer worse than the best member alone. A
`self_moa` mode samples a single strong model N times instead of using a mixed panel, for
profiles where one model clearly dominates.

### Judge and synthesis

A judge produces a structured analysis (consensus, contradictions, unique insights, blind
spots) over the panel answers, and a synthesizer writes the final answer grounded in it.
Synthesis generates a new answer; it is not a vote or a choose-from-N selection, because LLMs
are weak discriminative judges and selection can underperform plain decoding.

The dominant MoA failure mode is social or herding bias in the aggregator, and a stronger
aggregator does not fix it. The judge and synthesis steps are hardened structurally:

- Source anonymization: answers are presented without model identities, to blunt
  self-preference and expertise bias. This is the primary mitigation.
- Length normalization: answers are length-normalized or capped, because verbosity alone
  biases the aggregator.
- Mandatory critical dissent: prompts instruct critical evaluation and state that references
  may be wrong and must not be replicated, and that disagreeing with a majority is expected.
- Single-source dominance cap: the synthesis must not let any one source dominate, since a
  single deceptive or low-quality member can otherwise nullify the gains.
- Synthesizer not in its own panel, as defense in depth alongside anonymization.

### Usage accounting

Tokens spent across the router, panel, judge, and synthesis are aggregated into the response
`usage`, and emitted as per-model and per-stage metrics. The cost of a fused answer is never
hidden.

## Layers

The pipeline supports multiple refine layers, but the default is a single layer. Layer 1 answers
the raw query; each subsequent layer re-runs the full panel with the previous layer's anonymized
answers as reference, so members refine rather than start fresh. The final layer feeds the judge
and synthesizer.

Because every layer is a full panel fan-out, token cost and call count scale linearly with depth:
N layers spend N times the panel cost (plus one judge and one synthesis call), and this shows up
directly in the aggregated `usage`. Latency scales with depth as well, since layers run in
sequence. Depth also increases the surface for a bad member's influence to propagate, so layers
are bounded (1 to 4) and additional layers are reserved for offline or high-stakes paths. The
quality side of the curve is a benchmark target tracked in issue #20, not a number asserted here.

## Failure handling

- Router failure: fail open to fusion.
- Panel member timeout or error: dropped; proceed if quorum survived, else return an error.
- Judge failure: degrade to synthesis over the raw anonymized answers.
- Backend errors: surfaced in the OpenAI error shape.
- A bounded overall deadline cancels outstanding work.

## Evidence base

Findings from a 2024-2026 review (adversarially verified). Each decision is tied to its
evidence.

Validated, kept as-is:

- Single-layer panel, structured-judge, synthesis is the sound default for a latency-sensitive
  proxy; depth scales quality but multiplies cost and propagates a bad member's influence (MoA,
  arXiv 2406.04692).
- Synthesis is preferred over selection/voting; LLMs are weak discriminative judges and
  choose-from-N or self-refine can underperform greedy decoding (arXiv 2503.04104).
- Token-level fusion is out of scope for a multi-vendor OpenAI proxy (needs shared vocab and
  same-host logits).

Required refinements, folded in:

- Router gate as the primary cost lever; routing retains most quality at much lower cost, with
  thresholds calibrated per traffic (RouteLLM, arXiv 2406.18665).
- Proposer-quality floor and Self-MoA; diversity helps only among comparable-strength models,
  and Self-MoA can beat a mixed panel (arXiv 2502.00674).
- Aggregator hardening against conformity, self-preference, length, and single-source
  deception biases; a stronger aggregator does not fix these (arXiv 2410.12428, 2410.21819,
  2604.06091, 2503.05856).

Refuted, deliberately not built:

- In-panel confidence-weighted aggregation (not established; confidence is used only in the
  router gate).
- Pure choose-from-N selection / voting (propagates majority errors).

## Non-goals

- Dynamic per-query panel composition (panels are static per profile; the per-query decision
  is the fuse-or-not router gate).
- Token-level / logit fusion.
- A pure voting aggregator.
- In-panel confidence weighting.
- Calling model providers directly.
