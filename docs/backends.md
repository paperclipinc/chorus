# Backends

chorus speaks the OpenAI chat-completion contract to exactly one upstream, set by
`[backend].base_url` and the bearer key named by `[backend].api_key_env`. The same binary
runs unchanged against anything that exposes that contract. chorus never calls model
providers directly: it inherits the upstream's auth, routing, and logging, and treats the
key as opaque.

Two things follow from "one backend":

- A panel of distinct models needs an upstream that serves more than one model (Ollama, a
  hosted API, or a gateway). A single-model server (a lone vLLM instance) runs a self-MoA
  panel instead: one model sampled several times with varied temperature.
- The model ids in a profile are whatever that upstream names them. There is no chorus-side
  model registry.

Each section below has a complete, validated config under [`examples/`](../examples/). Copy
one, set the key env var, and run:

```bash
export CHORUS_BACKEND_KEY=...
CHORUS_CONFIG=examples/<backend>.toml cargo run -p chorus-server
```

The example configs are checked in CI: `chorus-server/tests/example_configs.rs` loads every
file in `examples/` through the same figment path the server uses and asserts it parses and
passes `Config::validate`. A live smoke test against a running backend is not run in CI
because it would need that backend provisioned; the per-backend run command above is the
manual smoke test.

## vLLM

A vLLM server serves a single model per instance at `/v1`. Because chorus talks to one
backend, the panel on a single vLLM is self-MoA: it samples the one served model several
times with varied temperature for diversity. For a panel of distinct models on vLLM, run
several vLLM instances behind a gateway and use the gateway config.

```bash
vllm serve meta-llama/Llama-3.1-8B-Instruct --api-key "$CHORUS_BACKEND_KEY"
```

Config: [`examples/vllm.toml`](../examples/vllm.toml). `base_url = "http://localhost:8000/v1"`.
The key must match vLLM's `--api-key` if one is set; otherwise any value works.

## Ollama

Ollama exposes an OpenAI-compatible endpoint at `/v1` and serves many models from one
process, so a real multi-model panel works against a single backend. Ollama does not require
an API key; set `api_key_env` to any variable holding a non-empty placeholder, since the
endpoint ignores the value.

```bash
ollama pull llama3.1 && ollama pull qwen2.5 && ollama pull mistral
export CHORUS_BACKEND_KEY=ollama
```

Config: [`examples/ollama.toml`](../examples/ollama.toml).
`base_url = "http://localhost:11434/v1"`.

## Hosted OpenAI-compatible API

Any hosted endpoint that speaks the OpenAI chat-completion contract works: point `base_url`
at it, name the env var that holds the bearer key, and use the model ids that provider
exposes.

Config: [`examples/openai-compatible.toml`](../examples/openai-compatible.toml).
`base_url = "https://api.your-provider.example/v1"`.

## Gateway (for example Bifrost)

A gateway gives chorus one OpenAI-compatible endpoint that fans out to many providers and
local servers behind it. This is the most flexible panel setup: members can be models from
different providers, all reached through the single backend URL, with the gateway owning the
provider keys and routing. chorus still holds only the gateway's key. The example pairs this
with the classifier router, so easy queries go to a single model and only hard ones fuse.

Config: [`examples/gateway.toml`](../examples/gateway.toml).
`base_url = "http://localhost:8080/v1"`.
