# engram

[![CI](https://github.com/cmjcode/engram/actions/workflows/ci.yml/badge.svg)](https://github.com/cmjcode/engram/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/cmjcode/engram)](https://github.com/cmjcode/engram/releases)
[![License: AGPL-3.0](https://img.shields.io/badge/license-AGPL--3.0-blue.svg)](LICENSE)
[![MCP Registry](https://img.shields.io/badge/MCP_Registry-io.github.cmjcode%2Fengram-6f42c1)](https://registry.modelcontextprotocol.io/?q=io.github.cmjcode/engram)

🇬🇧 English · [🇮🇩 Bahasa Indonesia](README.md)

**engram** is cross-tool long-term memory for AI coding agents and for the humans behind them:
architecture decisions, bug root causes, working conventions, down to life stories and family trees.
Everything is stored locally (embedded SurrealDB, one Rust binary) and served over MCP to
Claude Code, Codex CLI, Cursor, Windsurf, Antigravity, Gemini CLI and VS Code.

> *Bitemporal, evidence-backed, local-first memory for coding agents and life stories.*
> Not the same project as the Go/SQLite "Engram" in the MCP registry; this one focuses on
> time-stamped facts, an entity graph and principles backed by evidence.

## Why engram (what sets it apart from other MCP memory servers)

| Capability | engram | Typical memory server |
| --- | --- | --- |
| **Bitemporal facts** | Every fact has `valid_from`/`valid_to`; tool results separate *current* from *past*, and `as_of` shows memory as it was on a given date | New facts overwrite old ones without a trace, or only a storage timestamp |
| **Entity graph + kinship** | People, projects, components, tools, organisations and places are aliased entities; derived kinship (cousin, in-law) is computed from told relations | Embedded text chunks, no entities or relations |
| **Evidence-backed principles** | `principles` only returns decision patterns supported by several real episodes, cited as `[ep:ID]` | No "wisdom" layer, only facts |
| **Non-destructive compaction** | `compact` summarises clusters of episodes into a canonical one; originals are marked *retelling*, evidence stays intact and traceable | Summaries overwrite sources, or no compaction at all |
| **Ready-made project context** | `project_context` assembles active architecture facts, decisions, bug fixes, principles and related sessions for one repository at the start of a coding session (injected automatically by a `SessionStart` hook) | The agent has to assemble it from search results |
| **Filtering before storage** | A scoring pipeline separates chit-chat from decisions and important facts; its quality is measured with `engram bench eval` | Everything goes in as is |
| **Data sovereignty** | Local, open format, AGPL; optional sync to your own Google Drive | Vendor cloud, subscription |

MCP compliance: stateless Streamable HTTP (spec 2026-07-28; clients speaking 2024-11-05 through 2025-11-25 are
accepted), `tools/list` with `title`, `annotations` (`readOnlyHint` etc.) and `outputSchema` for every tool,
`structuredContent` in results, plus a `coding` tool profile so coding sessions are not flooded with life-memory tools.

## Install

macOS / Linux, installs the `engram` CLI and the `engram` desktop app:

```sh
curl -fsSL https://raw.githubusercontent.com/cmjcode/engram/main/install.sh | sh
```

Options: `--cli-only`, `--desktop-only`, `--version 0.1.2`, `--bin-dir ~/bin`, `--appimage` (Linux), e.g.
`curl -fsSL .../install.sh | sh -s -- --cli-only`. The CLI goes to `~/.local/bin`; the macOS app to `/Applications`,
Linux via `.deb`/`.rpm` (or AppImage without sudo). Windows: download the `.exe`/`.msi` from
[Releases](https://github.com/cmjcode/engram/releases).

**Docker / MCP Registry.** The same binary is published as `ghcr.io/cmjcode/engram` and listed in the official
MCP Registry as `io.github.cmjcode/engram` (`server.json`), so registry-aware clients can install it directly.

```sh
# server: UI, REST API and MCP Streamable HTTP on :7878
docker run -d --name engram -p 127.0.0.1:7878:7878 -v engram-data:/data -e MEMORY_TOKEN="$(openssl rand -hex 24)" ghcr.io/cmjcode/engram
# stdio MCP server for a client that spawns its own process
docker run -i --rm -v engram-data:/data ghcr.io/cmjcode/engram mcp --profile coding
```

On first start the container copies `memory.toml.example` to `/data/memory.toml`; point `[llm]` and `[embedding]`
at your OpenAI-compatible servers (vLLM, TEI, Ollama, …) or set `provider = "hash"` for an offline trial.

**Auto update.** The desktop app checks for a new release shortly after launch and every 6 hours, or via the tray
menu **Check for updates**. Update packages are signed (Tauri updater key). The CLI does not self-update yet; re-run
`install.sh`.

## Agent integration (MCP, rules and hooks)

`engram` connects to **Antigravity (IDE & CLI)**, **Gemini CLI**, **Claude Code**, **Cursor**, **Codex CLI**,
**Windsurf** and **VS Code**, so every important conversation is stored automatically and past context can be
pulled back at any time:

```sh
# Install the MCP server, a global rule and hooks into every client found on this machine
./scripts/install-mcp.sh --client all

# Or one client: antigravity | gemini | claude | cursor | codex | windsurf
./scripts/install-mcp.sh --client codex

# Override the tool profile (default: coding for cursor/codex/windsurf, full for the rest)
./scripts/install-mcp.sh --client cursor --profile full

# Remove the integration from every client
./scripts/install-mcp.sh --uninstall --client all
```

The script runs on macOS and Linux. On Windows use the **Integrations** panel in the desktop app, which does the
same through `engram`'s built-in installer (`crates/memory-server/src/install.rs`, also exposed as
`POST /api/mcp/clients/{id}/install`).

What `scripts/install-mcp.sh` sets up:

1. **MCP server (`engram`)** exposing `project_context`, `recall`, `remember`, `who_is`, `timeline`, `get_episodes`,
   `principles`, `persona_context`, `family_tree`, `memory_gaps`, `list_subjects` and `compact` over Streamable HTTP
   (`http://127.0.0.1:7878/mcp`) with a fixed `MEMORY_TOKEN`. Append `?profile=coding` (or `engram mcp --profile coding`
   for stdio) to load only the seven coding-session tools: `project_context`, `recall`, `get_episodes`, `remember`,
   `who_is`, `timeline`, `principles`. Codex uses `~/.codex/config.toml` (`[mcp_servers.engram]`), Windsurf
   `~/.codeium/windsurf/mcp_config.json`.
2. **Global rule** (`GEMINI.md`, `CLAUDE.md`, `AGENTS.md`, `global_rules.md`): instructs the agent to call
   `project_context`/`recall` at the start of a task and `remember` after important decisions. Coding clients get a
   compact variant of the rule.
3. **Hooks**: `Stop` in Antigravity and Claude Code, `AfterAgent` in Gemini CLI store each turn; `SessionStart` in
   Claude Code and Gemini CLI injects the project's memory index as initial context. When the server is down, events
   are spooled and replayed on the next call; conversations fall back to `vault/inbox/*.md` for later import.

### Agent session memory (`engram hook`)

Beyond MCP tools, engram records **work sessions** automatically through the thin hook
`engram hook <client> <event>` (same binary, never opens the vault, always exits 0):

| Claude Code hook | engram event | Purpose |
|---|---|---|
| `SessionStart` | `context` | injects the project's memory **index** (ID, date, type, title, token estimate), ≤ `inject.max_chars` |
| `UserPromptSubmit` | `session-init` | records the session, prompt number and prompt (after the privacy filter) |
| `PreToolUse` (Read/Edit/Write) | `file-context` | file history from memory, once per session per file; never blocks |
| `PostToolUse` / `PostToolUseFailure` | `observation` | one tool call into the turn spool (trimmed to 64 KB, images dropped, secrets redacted) |
| `Stop` / `PreCompact` | `summarize` | the turn becomes an `agent_session` source; the LLM extracts typed observations (`decision`, `bugfix`, `discovery`, `feature`, `refactor`, `change`), a session summary and project **lessons** |
| `SessionEnd` | `session-end` | closes the session |

The project is identified from the **git repository root** (worktrees map to their parent repo), not the folder name.
Agents retrieve memory progressively: `recall` (compact index) → `timeline(anchor)` → `get_episodes(ids)`; every tool
honours a token `budget`. `<private>…</private>` spans in prompts are never stored, and secrets (API keys, tokens, PEM
blocks, credentials in URLs) are replaced by `<redacted type='…'/>` before they reach the vault.

```sh
engram import-claude-memory --all --apply   # Claude Code auto-memory (~/.claude/projects/*/memory/*.md) -> episodes, no LLM
engram repo export <repo> [--project X]     # write <repo>/.engram/ (lessons, decisions, facts) to commit and share
engram repo import <repo>                   # merge a teammate's bundle into this vault (never deletes)
```

## Benchmark

Numbers from `engram bench` (episode writes + vector/BM25 search latency on embedded SurrealDB RocksDB, random
vectors, no LLM) on a development machine; rerun on yours with the same command:

```sh
engram --config memory.toml bench --episodes 20000 --queries 200
```

5 Nov 2026, Apple Silicon (macOS), engram 0.0.16, 20,000 episodes, 1024-dim vectors, HNSW index:

| Measure | Result |
| --- | --- |
| Episode writes | 16.8 s total, 1,188 episodes/s (down from 1,728/s over the first 5,000 as the index compacts) |
| Vector search top-10 | p50 33.0 ms, p95 45.7 ms, p99 1,836 ms (p99 while HNSW was still catching up with writes) |
| BM25 top-10 | p50 998 ms, p95 1,320 ms |
| Size on disk | 470.9 MB |

Filtering quality (do important fragments pass and chit-chat get dropped) is measured separately with a self-made
gold set: `engram bench export-gold --out cases/`, label `important` in `gold.toml`, then `engram bench eval cases/`
(precision/recall/leak) and `engram bench tune cases/` to search for the best `[score]` thresholds offline.

## Related products

- **AI legacy & biographer apps** (HereAfter AI, StoryFile, Eternos, Personal.ai) record life stories for families,
  but run as proprietary cloud services with simple Q&A retrieval and no temporal model of how a person changes.
- **Lifelogging** (Limitless, Omi) captures everything in real time; great for "what did I discuss this morning",
  poor at distilling decades of history or extracting principles from the noise.
- **Developer memory infrastructure** (Zep/Graphiti, Cognee, Mem0, Letta, Supermemory) is the closest technically.
  Zep and Cognee have temporal knowledge graphs; Mem0 and Letta are strong at agent state. None combine a bitemporal
  graph, evidence-backed principles, local-first storage and ready-made coding-session context in one self-hosted binary.

engram's niche: data sovereignty for 50+ years, a *wisdom layer* (principles, not just facts), and first-class support
for Indonesian storytelling and extended-family kinship, while remaining a drop-in MCP memory for coding agents.

## Development

```sh
cargo build --release -p memory-cli
cargo test --workspace --exclude memory-desktop
```

See [CONTRIBUTING.md](CONTRIBUTING.md), [SECURITY.md](SECURITY.md) and [CHANGELOG.md](CHANGELOG.md).
The Indonesian [docs/PANDUAN.md](docs/PANDUAN.md) covers configuration (`memory.toml`), the desktop app and the CLI
in depth.

## License

[GNU Affero General Public License v3.0 or later](LICENSE). Commercial licensing on request.
