# Contributing to engram

Thanks for your interest. engram is a Rust workspace (SurrealDB embedded, Axum, Tauri) that
serves long-term memory to AI coding agents over MCP. Issues and pull requests are welcome in
English or Indonesian.

## Development setup

```sh
git clone https://github.com/cmjcode/engram.git
cd engram
cp memory.toml.example memory.toml       # point [llm] and [embedding] at your servers,
                                          # or set provider = "hash" for offline tests
cargo build --release -p memory-cli       # CLI + server + MCP
./target/release/engram serve             # UI, REST API and MCP at http://127.0.0.1:7878
```

Desktop app (optional): `cargo install tauri-cli --version "^2" --locked`, then
`cargo run -p memory-desktop`. Linux needs the WebKitGTK packages listed in
`.github/workflows/release.yml`.

## Before opening a pull request

Run exactly what CI runs:

```sh
cargo fmt --all --check
cargo clippy --workspace --exclude memory-desktop --all-targets
cargo test --workspace --exclude memory-desktop
```

- Keep changes focused. One topic per PR.
- Add or update tests next to the code (`src/tests.rs` in each crate). MCP contract tests live in
  `crates/memory-server/src/mcp.rs`; any new tool needs `title`, `annotations`, `outputSchema`
  and a decision whether it belongs to the `coding` profile (`CODING_TOOLS`).
- The MCP surface is a public API. Renaming or removing a tool or a field in `structuredContent`
  is a breaking change; mention it in `CHANGELOG.md`.
- Do not commit anything from `vault/`, `eval/`, `memory.toml` or credentials. `build.sh` reads
  signing secrets from the environment or the Keychain, never from the repository.
- The UI (`crates/memory-server/ui`) is plain HTML/CSS/JS embedded in the binary. Use the
  existing SVG icon set rather than Unicode glyphs for icons.

## Commit messages

Short imperative subject, optional body explaining *why*. Releases are made with `build.sh <version>`
(macOS, by a maintainer) which also tags the commit; CI builds Windows, Linux and the Docker image
from that tag and publishes the version to the MCP Registry.

## Adding a new agent client

1. Add the client to `Client` in `crates/memory-server/src/install.rs` (config path per OS, JSON
   or TOML shape, default tool profile) and to `scripts/install-mcp.sh`.
2. Add a short rule block if the client supports a global instructions file.
3. If the client supports hooks, map its events in `crates/memory-server/src/hooks.rs`.
4. Document it in both `README.md` and `README.en.md`.

## License

By contributing you agree that your contributions are licensed under the
GNU Affero General Public License v3.0 or later (see `LICENSE`).
