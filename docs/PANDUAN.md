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

Paling mudah: di aplikasi desktop, klik ikon roda gigi (Pengaturan) > tab **Agent AI**, lalu **Pasang** pada
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

## Pengaturan, tentang & update

Klik ikon roda gigi di kanan atas UI:

- **Umum**: tema (ikuti sistem/terang/gelap), subjek default, dan (desktop) tombol buka folder
  data, inbox, dan konfigurasi.
- **Data & Sync**: lokasi data dan sinkronisasi Google Drive (lihat di bawah).
- **Agent AI**: pasang server MCP ke Antigravity, VS Code, atau Claude Code.
- **Layanan** (desktop): status server, worker, penjadwal, dan pemantau inbox; jalankan lagi yang berhenti.
  Titik merah di ikon roda gigi = ada layanan berhenti; titik hijau = ada update baru.
- **Model AI**: alamat server & model LLM, embedding, dan transkripsi Whisper, termasuk API key.
  Tombol *Tes koneksi* menguji nilai di formulir sebelum disimpan dan menampilkan daftar model di server.
  Ganti model/dimensi embedding: jalankan `engram reembed` setelah dibuka ulang.
- **Pemrosesan**: jumlah pekerja, interval inbox, proses ulang sumber gagal, umur obrolan ringan,
  jumlah hasil pencarian, batas upload.
- **Tentang & update**: versi, lisensi, lokasi data & `memory.toml`, tautan proyek, *Periksa update*.
  Di aplikasi desktop update bisa langsung dipasang, dan cek otomatis bisa dimatikan.
  Di `engram serve` hanya ditampilkan tautan unduhan.

Pengaturan disimpan ke `memory.toml` (kunci lain tidak diubah) dan berlaku setelah aplikasi dibuka
ulang; tombol *Buka ulang sekarang* muncul di aplikasi desktop. API key tidak pernah dikirim balik ke UI;
`api_key_env` (env var) tetap didahulukan bila terisi.

## Lokasi data

Semua memori (database, berkas sumber, inbox, ekspor) ada di satu folder vault.

- Aplikasi desktop: Pengaturan > **Data & Sync** > *Lokasi data* > pilih folder > *Pindahkan data*.
  Aplikasi dibuka ulang, data disalin ke folder baru, dan folder lama tidak dihapus
  (hapus sendiri bila semua sudah berjalan). Bila folder yang dipilih sudah berisi data
  engram, data itu yang dipakai.
- CLI: hentikan `engram serve`/`worker`, lalu `engram vault move /folder/baru`
  (`engram vault where` menampilkan lokasi sekarang).
- Jangan taruh vault di folder Google Drive/Dropbox/OneDrive/iCloud: database bisa rusak
  karena disalin setengah jadi. Untuk berbagi antar komputer pakai Sinkronisasi di bawah.

## Sinkronisasi antar komputer (Google Drive)

1. Pasang **Google Drive for desktop** di setiap komputer dan login ke akun yang sama.
2. Di komputer pertama: Pengaturan > **Data & Sync** > pilih folder di *My Drive* (mis. `My Drive/engram-sync`),
   isi kata sandi enkripsi, *Simpan*, lalu *Sync sekarang*.
3. Di komputer lain: pilih folder yang **sama** dan kata sandi yang **sama**, lalu *Sync sekarang*.

Sesudahnya sync berjalan otomatis sesuai jadwal (default tiap 10 menit) selama aplikasi atau
`engram serve`/`worker` berjalan. CLI: `engram sync setup --dir <folder> --passphrase <sandi>`,
`engram sync run`, `engram sync status`.

Cara kerjanya: database tetap di lokasi data lokal. Tiap komputer menulis salinan terenkripsi
miliknya ke `devices/<id>/` di folder sync, dan berkas sumber ke `sources/`. Saat sync, data
komputer lain digabungkan per record:

- record baru ditambahkan; daftar ID (mis. episode milik satu tokoh) digabung;
- bila dua komputer mengubah record yang sama, versi `updated_at` terbaru menang, selain itu
  versi yang ditarik menang;
- sumber yang belum selesai diproses di komputer asalnya ikut setelah selesai (tidak diproses dua kali);
- antrean job tetap lokal; **penghapusan tidak ikut tersinkron**;
- embedding dihitung ulang di komputer penerima (ditunda bila server embedding belum hidup).

Kata sandi disimpan di `<vault>/sync.key` (atau env `ENGRAM_SYNC_PASSPHRASE`). Lupa kata sandi
berarti salinan di Drive tidak bisa dibuka; data lokal tetap aman.

## Kedaulatan data

- `engram export` membuat JSONL + Markdown yang bisa dibaca tanpa software ini.
- `engram backup backup.tar.gz --recipient age1...` membuat backup terenkripsi.
- `engram restore backup.tar.gz.age --identity kunci.txt` memulihkan ke vault baru.
- Ganti model embedding: ubah `[embedding]`, lalu `engram reembed`.
- `engram bench --episodes 100000` mengukur kecepatan sebelum data membesar.
