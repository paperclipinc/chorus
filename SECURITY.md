# Security Policy

## Reporting a Vulnerability

Please do not file public issues for vulnerabilities.

- **Preferred**: GitHub private vulnerability reporting. Go to the Security tab of this
  repository and click "Report a vulnerability". MAINTAINER TODO: enable private
  vulnerability reporting in the repository Security settings if it is not already on.
- **Fallback**: email the security contact. MAINTAINER TODO: confirm or replace this
  address: `security@paperclip.inc` (placeholder; currently routes to `jannes@paperclip.inc`).

We will acknowledge your report within 72 hours and keep you informed of progress toward a
fix and disclosure.

### Response targets

These are targets we aim for on a valid, in-scope report; they are targets for a pre-1.0
project run by a small team, not a contractual SLA.

- Acknowledge receipt: within 72 hours.
- Initial severity assessment and triage: within 7 days.
- Fix or mitigation for a critical issue (key disclosure, prompt/response leakage):
  prioritized ahead of feature work; coordinated-disclosure timeline agreed with the
  reporter, default 90 days.
- Public disclosure: coordinated with the reporter after a fix ships, with credit unless
  the reporter requests anonymity.

## Supported Versions

This project is pre-1.0. Only the latest tagged release receives security fixes. There is no
backport window for older pre-1.0 tags: upgrade to the latest release to pick up a fix.

| Version | Supported |
|---|---|
| Latest tagged release | Yes |
| Any older pre-1.0 tag | No |

## Scope

chorus is a gateway that proxies chat-completion traffic to an upstream and holds an upstream
API key. The following are explicitly in scope:

- Disclosure of the configured backend API key or any tool credential (in logs, error
  messages, span fields, metrics, or responses).
- Leakage of prompt or completion content to anywhere it should not go (logs by default,
  metrics, cross-request bleed).
- Server-side request forgery or unintended egress via the backend base URL or a tool
  adapter target.
- Fan-out amplification abuse: a single request spends N backend calls, so a missing
  concurrency or quorum bound is a denial-of-service surface.
- The config loop guard: a profile whose panel, judge, or synthesizer references another
  fusion alias could recurse; bypassing the guard is in scope.

## Verifying releases

The published `chorus-server` image (`ghcr.io/paperclipinc/chorus`) is signed with cosign in
keyless mode using the publish workflow's GitHub OIDC identity, and carries an SPDX SBOM
attestation. There is no long-lived signing key. Verify the signature (replace `VERSION`
with the release tag, for example `v0.1.0`):

```bash
COSIGN_EXPERIMENTAL=1 cosign verify \
  --certificate-identity-regexp "https://github.com/paperclipinc/chorus/.github/workflows/publish.yaml@.*" \
  --certificate-oidc-issuer "https://token.actions.githubusercontent.com" \
  ghcr.io/paperclipinc/chorus:VERSION
```

A successful verify pins the signer to OUR publish workflow on OUR repository and exits 0; a
signature from any other identity fails.

## Code review policy for security-sensitive paths

Changes to the security-sensitive paths (the backend client `chorus-core/src/backend`, the
config loop guard `chorus-core/src/config`, and any tool adapter `chorus-core/src/tools`)
require a named human reviewer before merge, enforced by `.github/CODEOWNERS`.

## Current Status

This project has not yet had an external security review. Read the threat notes in the docs
before deploying it anywhere that matters.

## AI-Assisted Development Policy

Substantial portions of this codebase are AI-assisted. Security-sensitive paths receive
named-human review before merge, as listed above and in `.github/CODEOWNERS`.
