# Contributing

Thanks for your interest in contributing.

## Build and test

See the Commands section of [CLAUDE.md](CLAUDE.md) for the full list. The short version:

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

Integration tests run against a `wiremock` mock backend, so no network access or real models
are required.

## Commits

- Use conventional commits: feat, fix, docs, ci, chore, refactor, test.

## Developer Certificate of Origin (DCO)

Every commit must be signed off under the
[Developer Certificate of Origin](https://developercertificate.org/). Sign off with:

```bash
git commit -s
```

This appends a `Signed-off-by: Your Name <you@example.com>` trailer. By signing off you
certify that you wrote the change or otherwise have the right to submit it under the
project's open-source license (Apache 2.0). The sign-off identity must match the commit
author.

A lightweight check verifies that every commit in a pull request carries a sign-off. If you
forgot to sign off, add it to existing commits with:

```bash
git rebase --signoff origin/main
```

We use the DCO, NOT a Contributor License Agreement. This is the open-core-friendly choice:
it certifies provenance without assigning copyright or relicensing rights. See
[docs/open-core.md](docs/open-core.md).

## Named-human review for security-sensitive paths

Substantial portions of this codebase are AI-assisted. Changes to security-sensitive paths
require a named human reviewer before merge, in addition to CI. The paths in scope are the
backend client (`chorus-core/src/backend`), the config loop guard
(`chorus-core/src/config`), and any tool adapter (`chorus-core/src/tools`). The rule is
enforced by [`.github/CODEOWNERS`](.github/CODEOWNERS).

If you find a security vulnerability, do NOT open a public issue or PR; follow the
disclosure process in [SECURITY.md](SECURITY.md).

## Pull requests

- Tests for every behavior change, in the same commit.
- Docs updated in the same PR.
- If a quality, cost, or latency claim changed, include the benchmark run that backs it.
- All CI checks must be green: test, lint, deny, docker-build, codeql, scorecard.

## Where to start

- Issues labeled "good first issue".
- ROADMAP.md is the priority order; pick something near the top.

## Style

No em or en dashes anywhere; see the Coding Conventions section of [CLAUDE.md](CLAUDE.md).

## Licensing, open-core, and trademark

This repository is open source under the Apache License 2.0 (see `LICENSE`). Contributions
are accepted under the DCO and stay under that license. The open-core boundary is described
in [docs/open-core.md](docs/open-core.md). The "chorus" name and marks are reserved; see
[TRADEMARKS.md](TRADEMARKS.md). The code license does not grant trademark rights.
