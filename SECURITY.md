# Security Policy

## Reporting a vulnerability

Please **do not** open a public issue for security problems.

Report privately through GitHub: go to the repository's **Security** tab and choose **Report a vulnerability**. Include a description, the affected component and version, and steps to reproduce. If you have a proof of concept, attach it to the report rather than publishing it.

We aim to acknowledge reports within 3 working days and to agree on a disclosure timeline with you. We will credit you in the release notes unless you prefer otherwise.

## Scope

We are especially interested in:

- **End-to-end encrypted sync** — anything that lets the server, a holder of a login token, or a revoked device read, forge, roll back or hide records, or obtain a vault key. The design and its threat model are in [`docs/sync-design.md`](docs/sync-design.md).
- **The sync server** (`apps/cloud/`) — authentication, authorization between accounts, OAuth / PKCE handling, quota and rate-limit bypasses.
- **The macOS app** — anything that lets another process abuse Qantrip's Accessibility permission, leak prompts to clipboard history or logs, or execute code through templates (rendering is sandboxed and must not access files, network or processes).

Out of scope: denial of service against the hosted service, issues that require an already compromised device, and missing hardening headers without a demonstrated impact.

## Supported versions

Qantrip is pre-1.0. Security fixes are released for the latest version only.
