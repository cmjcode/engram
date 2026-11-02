# Panduan engram

Orkestrator memori berbasis Rust + SurrealDB embedded. Semua data ada di
satu folder `vault/` di komputer Anda.

## Mulai

```bash
cargo build --release -p memory-cli
cp memory.toml.example memory.toml          # sesuaikan endpoint vLLM/TEI
./target/release/engram init --id bapak --name "Bapak Sukarno" --birth 1955-03-10
./target/release/engram smoke               # cek koneksi LLM & embedding
./target/release/engram serve               # UI di http://127.0.0.1:7878
```

`serve` menjalankan UI web, API, MCP HTTP (`/mcp`), worker job, scheduler
(konsolidasi, distilasi, profil gaya tutur, ekspor harian), dan pemantau inbox.

## Aplikasi desktop (macOS)

```bash
cargo install tauri-cli --version "^2" --locked   # sekali saja
cd apps/memory-desktop && cargo tauri build       # hasil: target/release/bundle/macos/engram.app (+ .dmg)
```

Untuk mencoba tanpa membuat bundle: `cargo run -p memory-desktop`.

- Aplikasi berisi seluruh mesin (vault, worker, scheduler, inbox, server MCP). Tidak perlu `engram serve`.
- Data ada di `~/Library/Application Support/id.engram.ai/`: `memory.toml`, `vault/`, dan `connection.json`.
- Menutup jendela hanya menyembunyikannya; pemrosesan tetap jalan. Keluar lewat ikon di menu bar, lalu **Keluar**.
- Menu bar juga punya **Buka folder inbox** (seret berkas ke sana) dan **Buka folder data**.
- Agent AI bisa terhubung ke `mcp_url` di `connection.json` dengan header `Authorization: Bearer <token>`.
  Token dibuat sekali lalu disimpan di `mcp_token` (atau pakai env `MEMORY_TOKEN`).
- Aplikasi dan `engram serve` tidak bisa membuka vault yang sama bersamaan. Untuk memakai CLI pada data aplikasi,
  tutup aplikasi dulu lalu jalankan `engram --config ~/Library/Application\ Support/id.engram.ai/memory.toml ...`.

## Menambah memori

| Cara | Perintah / langkah |
|---|---|
| Seret berkas | UI, menu "Sumber & import" |
| Folder inbox | taruh berkas di `vault/inbox/` (subfolder `inbox/<id-subjek>/` untuk subjek lain) |
| Import massal | `engram import ./arsip/ --kind interview` |
| Wawancara | UI "Wawancara" atau `engram interview` |
| Dari agent AI | tool MCP `remember` |

Format: teks/Markdown, ekspor WhatsApp (.txt), Telegram (`result.json`),
ChatGPT/Claude (`conversations.json`), email (.mbox/.eml), foto + caption
(`foto.jpg` + `foto.txt`), audio (butuh `[transcription] base_url`).

Teks yang sama tidak pernah diproses dua kali. Basa-basi chat ("oke sip",
"wkwk") tidak menjadi memori jangka panjang.

### Sumber yang gagal diproses

Bila server LLM/embedding mati, sumber bisa berstatus `failed`. engram
memprosesnya ulang otomatis tiap `[schedule] retry_failed_minutes` (default 10)
**hanya bila server sudah bisa dihubungi**, maksimal `retry_failed_max` kali
(default 20) per sumber. Proses ulang manual:

- UI: tombol "Proses ulang" di detail sumber, atau "Proses ulang yang gagal" di Ringkasan
- CLI: `engram jobs retry` (semua) atau `engram jobs retry --source <id> --run`
- API: `POST /api/jobs/retry-failed`, `POST /api/sources/{id}/retry`

## Menghubungkan agent AI (MCP)

Paling mudah: di aplikasi desktop, klik tombol **Agent AI** di kanan atas, lalu **Pasang** pada
Antigravity, VS Code, atau Claude Code. Aplikasi menambah server `engram` ke config client itu
(`~/.gemini/antigravity/mcp_config.json`, `mcp.json` di folder User VS Code, atau `claude mcp add -s user`),
tanpa mengubah server lain, dan menyimpan cadangan `.bak`. Tombol yang sama ada di `engram serve` bila
server berjalan di 127.0.0.1.

Lewat terminal (Antigravity, Gemini CLI, Claude Code, Cursor):

```bash
scripts/install-mcp.sh                      # Antigravity, lewat HTTP ke aplikasi desktop
scripts/install-mcp.sh --client all         # semua client (MCP + Global Rule + Hook otomatis)
scripts/install-mcp.sh --mode stdio         # tanpa aplikasi desktop (jalankan binary engram)
scripts/install-mcp.sh --uninstall --client all
```

Selain mendaftarkan server MCP `engram`, script ini juga memasang:
- **Global Rule** (`~/.gemini/GEMINI.md`, `~/.gemini/config/GEMINI.md`, `~/.claude/CLAUDE.md`) agar agent otomatis menyimpan fakta/cerita hidup lewat `remember` dan mengambil konteks lewat `recall`/`who_is`/`timeline`/`principles`.
- **Hook Otomatis** (`~/.local/bin/engram-auto-remember` dari `scripts/auto-remember-hook.py`) pada event `Stop` (Antigravity & Claude Code) dan `AfterAgent` (Gemini CLI) yang mengirim percakapan baru ke MCP `remember` (atau ke `vault/inbox/` bila server sedang mati).

Mode HTTP membuat token tetap `MEMORY_TOKEN` (disimpan di `mcp_token` dan LaunchAgent
`id.engram.ai.token`, jadi tetap ada setelah reboot). Setelah pertama kali dijalankan, keluar
dari aplikasi lewat menu bar lalu buka lagi. Config lama selalu dibackup (`*.bak.<waktu>`).
`--dry-run` menampilkan rencana tanpa mengubah apa pun.

Manual, Claude Code:

```bash
claude mcp add engram -- /path/ke/engram --config /path/ke/memory.toml mcp
```

Atau lewat HTTP: `POST http://127.0.0.1:7878/mcp` (pakai header
`Authorization: Bearer $MEMORY_TOKEN` bila token diset).

Tool: `recall`, `who_is`, `timeline`, `principles`, `persona_context`,
`family_tree`, `remember`, `memory_gaps`, `list_subjects`.

## Melihat dan mengoreksi

- `engram recall "partai yang didukung"`; tambah `--as-of 2005-01-01` untuk "versi dirinya" saat itu.
- `engram who "Pakde Karno"`, `engram timeline --entity Karno`, `engram principles`.
- `engram review list` lalu `approve`/`reject`: konflik fakta, entitas kembar, data zona abu-abu.

## Kedaulatan data

- `engram export` membuat JSONL + Markdown yang bisa dibaca tanpa software ini.
- `engram backup backup.tar.gz --recipient age1...` membuat backup terenkripsi.
- `engram restore backup.tar.gz.age --identity kunci.txt` memulihkan ke vault baru.
- Ganti model embedding: ubah `[embedding]`, lalu `engram reembed`.
- `engram bench --episodes 100000` mengukur kecepatan sebelum data membesar.
