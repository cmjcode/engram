# Panduan Memori Hidup

Orkestrator memori hidup berbasis Rust + SurrealDB embedded. Semua data ada di
satu folder `vault/` di komputer Anda.

## Mulai

```bash
cargo build --release -p memory-cli
cp memory.toml.example memory.toml          # sesuaikan endpoint vLLM/TEI
./target/release/memory init --id bapak --name "Bapak Sukarno" --birth 1955-03-10
./target/release/memory smoke               # cek koneksi LLM & embedding
./target/release/memory serve               # UI di http://127.0.0.1:7878
```

`serve` menjalankan UI web, API, MCP HTTP (`/mcp`), worker job, scheduler
(konsolidasi, distilasi, profil gaya tutur, ekspor harian), dan pemantau inbox.

## Aplikasi desktop (macOS)

```bash
cargo install tauri-cli --version "^2" --locked   # sekali saja
cd apps/memory-desktop && cargo tauri build       # hasil: target/release/bundle/macos/Memori Hidup.app (+ .dmg)
```

Untuk mencoba tanpa membuat bundle: `cargo run -p memory-desktop`.

- Aplikasi berisi seluruh mesin (vault, worker, scheduler, inbox, server MCP). Tidak perlu `memory serve`.
- Data ada di `~/Library/Application Support/id.memorihidup.desktop/`: `memory.toml`, `vault/`, dan `connection.json`.
- Menutup jendela hanya menyembunyikannya; pemrosesan tetap jalan. Keluar lewat ikon di menu bar, lalu **Keluar**.
- Menu bar juga punya **Buka folder inbox** (seret berkas ke sana) dan **Buka folder data**.
- Agent AI bisa terhubung ke `mcp_url` di `connection.json` dengan header `Authorization: Bearer <token>`.
  Token dibuat acak setiap aplikasi dibuka (atau pakai env `MEMORY_TOKEN` agar tetap).
- Aplikasi dan `memory serve` tidak bisa membuka vault yang sama bersamaan. Untuk memakai CLI pada data aplikasi,
  tutup aplikasi dulu lalu jalankan `memory --config ~/Library/Application\ Support/id.memorihidup.desktop/memory.toml ...`.

## Menambah memori

| Cara | Perintah / langkah |
|---|---|
| Seret berkas | UI, menu "Sumber & import" |
| Folder inbox | taruh berkas di `vault/inbox/` (subfolder `inbox/<id-subjek>/` untuk subjek lain) |
| Import massal | `memory import ./arsip/ --kind interview` |
| Wawancara | UI "Wawancara" atau `memory interview` |
| Dari agent AI | tool MCP `remember` |

Format: teks/Markdown, ekspor WhatsApp (.txt), Telegram (`result.json`),
ChatGPT/Claude (`conversations.json`), email (.mbox/.eml), foto + caption
(`foto.jpg` + `foto.txt`), audio (butuh `[transcription] base_url`).

Teks yang sama tidak pernah diproses dua kali. Basa-basi chat ("oke sip",
"wkwk") tidak menjadi memori jangka panjang.

## Menghubungkan agent AI (MCP)

Claude Code:

```bash
claude mcp add memori -- /path/ke/memory --config /path/ke/memory.toml mcp
```

Atau lewat HTTP: `POST http://127.0.0.1:7878/mcp` (pakai header
`Authorization: Bearer $MEMORY_TOKEN` bila token diset).

Tool: `recall`, `who_is`, `timeline`, `principles`, `persona_context`,
`family_tree`, `remember`, `memory_gaps`, `list_subjects`.

## Melihat dan mengoreksi

- `memory recall "partai yang didukung"`; tambah `--as-of 2005-01-01` untuk "versi dirinya" saat itu.
- `memory who "Pakde Karno"`, `memory timeline --entity Karno`, `memory principles`.
- `memory review list` lalu `approve`/`reject`: konflik fakta, entitas kembar, data zona abu-abu.

## Kedaulatan data

- `memory export` membuat JSONL + Markdown yang bisa dibaca tanpa software ini.
- `memory backup backup.tar.gz --recipient age1...` membuat backup terenkripsi.
- `memory restore backup.tar.gz.age --identity kunci.txt` memulihkan ke vault baru.
- Ganti model embedding: ubah `[embedding]`, lalu `memory reembed`.
- `memory bench --episodes 100000` mengukur kecepatan sebelum data membesar.
