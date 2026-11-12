# Changelog

All notable changes to engram. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
versions follow SemVer (pre-1.0: minor bumps may break the MCP surface).

## [Unreleased]

## [0.4.0] - 2026-11-12

### Breaking
- **SurrealDB 4 + SurrealKV.** Storage moves from SurrealDB 3.3 (RocksDB) to SurrealDB 4.0 with the pure-Rust
  SurrealKV engine (RocksDB was removed in SurrealDB 4). The database now lives in `<vault>/db-skv/`; a vault
  that still has a v0.3 RocksDB `db/` refuses to open with a message explaining the migration.
  `vault.engine = "rocksdb"` is accepted as an alias of `"surrealkv"` with a warning.
- **Vault migration from v0.3:** with engram 0.3 run `engram backup vault-v3.tar.gz`, then with engram 0.4
  run `engram vault migrate-v3 vault-v3.tar.gz`. Records are restored, embeddings recomputed, and the old
  `db/` is renamed to `db-rocksdb-v3.bak/` (delete it once everything works). Hub tables (`hub_*`) are not part
  of the backup; a hub server re-creates them empty.

### Added
- Embedding engram in another Rust application: `memory_store::Store::from_db`, `memory_server::AppState::embedded`
  (no bearer token, for in-process calls of `memory_server::router`), and `memory-server` features `ui` (web UI)
  and `hub` (team hub), both on by default. `default-features = false` keeps only `/api/*`, `/mcp` and `/api/health`.
- Dual licence: AGPL-3.0-or-later, or the internal commercial licence in `LICENSE-COMMERCIAL`.
- **Team hub** (`engram serve --hub`, new crate `memory-hub`): shared project memory moves from the git
  repository to a server. Sign in with Google or GitHub (same verified email = one account), JWT access tokens
  with rotating refresh tokens, personal access tokens for CI (`ENGRAM_HUB_URL` + `ENGRAM_HUB_TOKEN`), roles
  owner / maintainer / member / reader, join key in `.engram/repo.toml` (hashed on the hub, rotatable),
  optional GitHub permission check, audit log, per-IP rate limits, and a web UI at `/hub`. The hub keeps a
  per-repo change log (`seq`): devices push only changed records and pull from their cursor. It never stores
  raw transcripts, episode text or embeddings, and rejects records that still contain redaction markers.
  Docker: `docker/docker-compose.yml` (built from source, joins an existing nginx network) and `docker/nginx-engram.conf`; `HEALTHCHECK` via `engram health`.
- CLI: `engram login [--hub URL] [--provider google|github]`, `engram logout`, `engram whoami`,
  `engram repo init | join [--github] | push [--full] [--dry-run] | pull | status | migrate | rotate-key |
  disconnect`. Teammates join automatically on their first agent session in a repo with `repo.toml`.
- Shared memory now includes project episode titles and summaries (`[repo_share] share_episodes`).

### Changed
- Dependencies aligned with OPERON: `reqwest` 0.13, `tower-http` 0.7.
- Team memory no longer writes `.engram/members/`, `README.md` or git hooks: the only committed file is
  `.engram/repo.toml`, so `git status` stays clean. `engram repo migrate` uploads existing bundles to the hub,
  removes them and the engram git hooks. Old bundles from teammates who have not migrated are still read
  (`legacy_import = true`), never written.
- Shared record IDs derive from content (lesson title key, decision situation + action, fact entity +
  predicate + object) instead of local IDs, so fact supersedes and episode compaction no longer create
  new records plus tombstones.
- MCP tool profile `operon` (`?profile=operon`, `engram mcp --profile operon`) for multi-tenant memory layers
  (one engram subject per tenant): `recall`, `remember`, `who_is`, `timeline`, `principles`, `project_context`,
  `get_episodes` only; no `family_tree`, `compact`, `memory_gaps`, `list_subjects`, `persona_context`.
- Per-subject tokens for tenant isolation: `MEMORY_SUBJECT_TOKENS=subjectA=tokenA,subjectB=tokenB`
  (`server.subject_tokens_env`). A subject token may only touch `/api/subjects/{s}/...` for its own subject,
  `POST /api/subjects` with the same id (`GET` lists only that subject), `/mcp` with the `subject` argument forced
  to it (`list_subjects` refused), and `/api/reviews/{id}` / `/api/sources/{id}` it owns; everything else answers
  `403`. The global `MEMORY_TOKEN` keeps full access.
- Score category `business_fact`: short, flat business statements ("Outlet Bandung tutup sejak September 2026",
  "Target omzet Q4 naik 15%") with two business terms, or one term plus a number, are promoted to the episode
  threshold instead of being dropped as small talk. Thresholds are unchanged; "ok makasih" is still dropped.
- Project groups: projects sharing a tag (e.g. `#VOLTUNES`) form a group. `project_context` for one
  repo now adds an "Ekosistem grup" section: the umbrella project's shared architecture, a map of every
  sibling repo (role + tech stack), cross-repo conventions and recent group sessions. Membership also
  comes from a `GROUP/repo` prefix or a parent folder named after the tag. `recall` with `project`
  keeps sibling repos in scope at lower priority; a group name covers the whole group. Opt out with
  `group: false`; extra budget via `retrieval.group_budget_tokens` (default 3000).
- `LICENSE` (AGPL-3.0-or-later), `CONTRIBUTING.md`, `SECURITY.md`, this changelog.
- `README.en.md`: English documentation.
- `server.json` for the official MCP Registry (`io.github.cmjcode/engram`), `Dockerfile` and
  `ghcr.io/cmjcode/engram` image; `docker.yml` publishes the image and the registry entry on each tag.
- `ci.yml`: fmt, clippy, tests, shellcheck and `server.json` validation on every push and PR.
- `scripts/install-mcp.sh` runs on Linux as well as macOS (token persisted via `~/.config/environment.d`).
- UI: "start all services" button; parallel backend health check.
- UI: "Setup .engram" button on project cards linked to a git repo (creates the `.engram/` team
  memory folder in one click); the projects API now reports each project's `.engram` state.
- Project metadata (schema v3, `project_meta` table): git repository (typed or auto-detected from
  `.git/config` of the session folder), notes with Obsidian-style `[[Project]]` links plus backlinks,
  and group tags. The Projects page filters and groups by tag; `project_context` now reports the
  repository, tags and related projects to agents. API: `GET/PUT /api/subjects/{s}/projects/meta`.
- Obsidian-style project graph: `[[Project]]` links written in remembered text and session observations
  are stored on the episode (`links`) and count as backlinks ("linked in memory") next to note backlinks.
  `GET /api/subjects/{s}/projects/graph?project=&depth=&tags=&entities=` returns project, tag and entity
  nodes with `link`, `mention`, `tag` and `entity` edges (global, or a local graph around one project).
  The UI adds a *Project graph* page and a *Graph* tab per project (canvas, pan/zoom/drag, search, filters,
  text adjacency list); `project_context` lists linked and mutually-mentioning projects.
- `engram projects remap <plan.json> [--apply]`: move episodes, sessions, sources and lessons of old
  data to the right project and add group tags; dry run by default, `null` clears a project.

- Team memory through the git repository: everyone with access to a repo shares its project lessons,
  decisions and facts via `<repo>/.engram/`. Bundle format v2 writes one canonical JSON file per record
  under `members/<name>-<hash>/` per device, so git merges never conflict; re-exporting unchanged memory
  writes nothing. Storage and search stay in embedded SurrealDB; the bundle carries distilled text only.
- Imports are non-destructive: shared decisions become decision episodes, shared facts go through normal
  reconciliation, and records withdrawn by their author arrive as tombstones (episode quarantined, fact
  closed with `valid_to`). Imported memory is never re-exported.
- Automation: SessionStart/SessionEnd queue `repo_import`/`repo_export` jobs, `engram repo hooks`
  installs `post-merge`, `post-checkout` and `pre-commit` git hooks (`engram hook git repo-sync|repo-export`),
  and the scheduler checks known repos (`[repo_share]` in `memory.toml`). engram never commits.
- CLI `engram repo init [--hooks] | status | export [--dry-run] | import | hooks [--remove]`, routed
  through the running server when there is one. UI panel "Memori tim lewat repo git" on the project
  page; API `GET/POST /api/subjects/{s}/projects/share`.
- `ingest_distilled` accepts known structure (`obs_type`, decisions, entities, facts, concepts) in
  `meta.extraction`.
- Project graph as a rotating 3D globe (default) with a 2D toggle, still offline and dependency-free
  (hand-written perspective projection on the existing canvas, code moved to `ui/graph.js`). Drag to
  rotate, Shift+drag or two fingers to pan, wheel or pinch to zoom; auto-rotation on the global graph.
- Graph detail panel: kind, memory count, last activity, tags, neighbours grouped by edge kind with the
  reason for each link, and *Open*, *Focus here* (local graph without leaving the page, Back works) and
  *Copy name* actions. Shareable URLs `#graf/<project>?depth=N`; "Open full graph" link on project pages.
- Graph colouring by node kind, group tag (with a clickable legend that highlights a group) or activity;
  "Active within 7/30/90 days" filter. Edge tooltips explain why two nodes are linked. Keyboard: arrows
  step through nodes, Enter opens, Esc closes, `/` search, `m` 2D/3D, `r` auto-rotate.
- Graph API nodes carry `last_at` (latest memory of a project; tags and entities inherit the newest of
  their projects).
- `scripts/ui-graph-smoke.mjs`: mock-DOM smoke test for the graph, run by `cargo test` when Node is present.

### Fixed
- Project attribution per git repository: a watched folder that holds many repos (e.g. `FOX/`,
  `VOLTUNES/`) is now summarised per repo, each under its repo name with the folder label as group tag,
  instead of one memory labelled with the folder name.
- Claude Code scratchpad folders (`/tmp/claude-*/<encoded project>/...`) map back to the session's real
  project, and OS temp folders (`/var/folders/.../T`) are no longer recorded as projects.

### Changed
- Project graph: clicking a node now selects it and opens the detail panel; double-click (or Enter)
  opens the project, tag or entity as before.
- `build.sh` no longer contains credentials; it reads them from the environment, `~/.config/engram/notarize.env`
  or the macOS Keychain.

## [0.1.2] - 2026-11-05
### Added
- UI: left sidebar with Global / Work / Personal navigation groups, responsive top bar.
### Fixed
- Tab indicator and race condition when switching menus quickly.

## [0.1.1] - 2026-11-05
### Changed
- UI refresh (sidebar layout, narrow-screen support).

## [0.0.16] - 2026-11-05
### Added
- Agent session memory via `engram hook <client> <event>` (SessionStart context injection, per-tool
  observations, Stop/PreCompact summaries, lessons per project).
- Installers for Cursor, Codex CLI and Windsurf; `coding` tool profile; profile badges in the UI.
- MCP spec 2026-07-28: `title`, `annotations`, `outputSchema`, `structuredContent` on every tool.
- `engram import-claude-memory`, `engram repo export|import`.
### Fixed
- Date/time handling.

## [0.0.15] - 2026-11-04
### Fixed
- Folder watch (inbox) reliability.

## [0.0.14] - 2026-11-03
### Added
- Inbox folder watch.

## [0.0.12] - 2026-11-03
### Added
- Non-destructive semantic compaction (`compact`), detail views.
- `engram bench eval|tune|export-gold`: gold-set evaluation and threshold tuning for the extraction pipeline.

## [0.0.11] - 2026-11-02
### Added
- LLM service management in the UI.

## [0.1.0] - 2026-11-02
### Fixed
- Embedding server status.

## [0.0.9] - 2026-11-02
### Added
- Encrypted Google Drive sync (`engram sync`), reprocess and service status.

## [0.0.7] - 2026-11-01
### Fixed
- Hook handling; LLM error variant in pipeline tests.

## [0.0.5] - 2026-10-31
### Added
- `engram` CLI.
### Changed
- Project renamed from "mnemonic-memory" to **engram**.

## [0.0.4] - 2026-10-31
### Added
- `scripts/install-mcp.sh`.

## [0.0.3] - 2026-10-31
### Added
- Desktop auto-update, new design, "install agent" panel.

## [0.0.2] - 2026-10-31
### Added
- `install.sh` one-line installer.

## [0.0.1] - 2026-10-31
- First release: Rust workspace, SurrealDB vault, extraction pipeline, UI, MCP server.

[Unreleased]: https://github.com/cmjcode/engram/compare/v0.1.2...HEAD
[0.1.2]: https://github.com/cmjcode/engram/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/cmjcode/engram/compare/v0.0.16...v0.1.1
[0.0.16]: https://github.com/cmjcode/engram/compare/v0.0.15...v0.0.16
[0.0.15]: https://github.com/cmjcode/engram/compare/v0.0.14...v0.0.15
[0.0.14]: https://github.com/cmjcode/engram/compare/v0.0.12...v0.0.14
[0.0.12]: https://github.com/cmjcode/engram/compare/v0.0.11...v0.0.12
[0.0.11]: https://github.com/cmjcode/engram/compare/v0.1.0...v0.0.11
[0.1.0]: https://github.com/cmjcode/engram/compare/v0.0.9...v0.1.0
[0.0.9]: https://github.com/cmjcode/engram/compare/v0.0.7...v0.0.9
[0.0.7]: https://github.com/cmjcode/engram/compare/v0.0.5...v0.0.7
[0.0.5]: https://github.com/cmjcode/engram/compare/v0.0.4...v0.0.5
[0.0.4]: https://github.com/cmjcode/engram/compare/v0.0.3...v0.0.4
[0.0.3]: https://github.com/cmjcode/engram/compare/v0.0.2...v0.0.3
[0.0.2]: https://github.com/cmjcode/engram/compare/v0.0.1...v0.0.2
[0.0.1]: https://github.com/cmjcode/engram/releases/tag/v0.0.1
