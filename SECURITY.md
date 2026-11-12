# Security policy

## Reporting a vulnerability

Please do not open a public issue for security problems. Use GitHub's private
[security advisory form](https://github.com/cmjcode/engram/security/advisories/new) for this
repository. You will get an acknowledgement within 7 days. Fixes are shipped as a new tagged
release and noted in `CHANGELOG.md`.

## Scope and threat model

engram runs **locally** on the user's machine. The data it stores (project decisions, bug root
causes, personal stories) is sensitive by design, so these properties matter:

- **Local only by default.** The server binds to `127.0.0.1:7878`. When `MEMORY_TOKEN` is set,
  every `/api` and `/mcp` request must carry `Authorization: Bearer <token>`. Binding to another
  address without a token logs a warning; do not expose the port without a token and TLS in front.
- **Per-subject tokens.** `MEMORY_SUBJECT_TOKENS=subjectA=tokenA,subjectB=tokenB` issues tokens that may
  only touch their own subject (`/api/subjects/{s}/...`, `/mcp` with `subject` forced, their own reviews and
  sources); admin endpoints answer `403`. Use one engram subject per tenant and never hand a tenant the
  global `MEMORY_TOKEN`.
- **Client configs contain the token.** The installers write MCP client configs with mode `0600`.
- **Secret redaction before storage.** Hooks redact API keys, tokens, PEM blocks and credentials in
  URLs (`<redacted type='…'/>`) and drop `<private>…</private>` spans before anything reaches the
  vault (`[privacy]` in `memory.toml`).
- **Sync is encrypted.** Optional Google Drive sync uses `age` with a passphrase
  (`ENGRAM_SYNC_PASSPHRASE`); plaintext never leaves the machine.
- **Release integrity.** macOS builds are signed and notarized; desktop updates are signed with the
  Tauri updater key; every release asset ships a `.sha256`.

Out of scope: vulnerabilities in the LLM or embedding servers you point engram at, and prompt
injection via content you deliberately import.

## Supported versions

Only the latest tagged release receives fixes.
