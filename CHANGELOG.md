# Changelog

All notable changes to engram. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/);
versions follow SemVer (pre-1.0: minor bumps may break the MCP surface).

## [Unreleased]

### Added
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

### Fixed
- Project attribution per git repository: a watched folder that holds many repos (e.g. `FOX/`,
  `VOLTUNES/`) is now summarised per repo, each under its repo name with the folder label as group tag,
  instead of one memory labelled with the folder name.
- Claude Code scratchpad folders (`/tmp/claude-*/<encoded project>/...`) map back to the session's real
  project, and OS temp folders (`/var/folders/.../T`) are no longer recorded as projects.

### Changed
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
