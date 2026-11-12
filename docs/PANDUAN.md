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
Antigravity, VS Code, Claude Code, Cursor, Codex CLI, atau Windsurf. Aplikasi menambah server `engram` ke
config client itu (`~/.gemini/antigravity/mcp_config.json`, `mcp.json` di folder User VS Code,
`claude mcp add -s user`, `~/.cursor/mcp.json`, `~/.codex/config.toml`, `~/.codeium/windsurf/mcp_config.json`),
tanpa mengubah server lain, dan menyimpan cadangan `.bak`. Tombol yang sama ada di `engram serve` bila
server berjalan di 127.0.0.1.

**Profil tool.** Client koding murni (VS Code, Cursor, Codex, Windsurf) didaftarkan dengan `?profile=coding`:
hanya `project_context`, `recall`, `remember`, `who_is`, `timeline`, `principles` yang dimuat, sehingga konteks
agent tidak terisi tool memori hidup (`family_tree`, `persona_context`, `memory_gaps`, `compact`, `list_subjects`).
Antigravity dan Claude Code memakai profil `full`. Untuk stdio: `engram mcp --profile coding`.
Profil `operon` (`?profile=operon`) untuk aplikasi multi-tenant yang memakai satu subjek per tenant: `recall`,
`remember`, `who_is`, `timeline`, `principles`, `project_context`, `get_episodes` saja; gabungkan dengan token per
subjek (`MEMORY_SUBJECT_TOKENS`, lihat di bawah).

Lewat terminal (Antigravity, Gemini CLI, Claude Code, Cursor, Codex, Windsurf):

```bash
scripts/install-mcp.sh                      # Antigravity, lewat HTTP ke aplikasi desktop
scripts/install-mcp.sh --client all         # semua client (MCP + Global Rule + Hook otomatis)
scripts/install-mcp.sh --mode stdio         # tanpa aplikasi desktop (jalankan binary engram)
scripts/install-mcp.sh --client codex                # Codex CLI (~/.codex/config.toml), profil coding
scripts/install-mcp.sh --client cursor --profile full # timpa profil bawaan
scripts/install-mcp.sh --uninstall --client all
```

Selain mendaftarkan server MCP `engram`, script ini juga memasang:
- **Global Rule** (`~/.gemini/GEMINI.md`, `~/.gemini/config/GEMINI.md`, `~/.claude/CLAUDE.md`; versi ringkas di `~/.codex/AGENTS.md` dan `~/.codeium/windsurf/memories/global_rules.md`) agar agent otomatis memanggil `project_context`/`recall` di awal tugas dan menyimpan keputusan lewat `remember`.
- **Hook Otomatis** (`~/.local/bin/engram-auto-remember` dari `scripts/auto-remember-hook.py`) pada event `Stop` (Antigravity & Claude Code) dan `AfterAgent` (Gemini CLI) yang mengirim percakapan baru ke MCP `remember` (atau ke `vault/inbox/` bila server sedang mati).
- **Hook SessionStart** di Claude Code dan Gemini CLI: menyuntik indeks konteks proyek untuk repositori yang sedang dibuka sebagai konteks awal sesi (`engram hook <client> context`; cadangan Python `scripts/session-context-hook.py` bila binary tidak ada).

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

**Token per subjek (isolasi tenant).** Selain token global, env `MEMORY_SUBJECT_TOKENS`
(`server.subject_tokens_env`) berisi `subjekA=tokenA,subjekB=tokenB`. Token subjek hanya boleh menyentuh
`/api/subjects/{s}/...` dengan `s` yang sama, `POST /api/subjects` untuk id yang sama, `/mcp` (argumen `subject`
dipaksa ke subjek itu, `list_subjects` ditolak), serta review/sumber milik subjek itu; endpoint admin (settings,
storage, update, services) dijawab `403`. Token global tetap bisa semua.

Transport HTTP mengikuti Streamable HTTP tanpa sesi (spesifikasi MCP 2026-07-28; versi 2024-11-05 s/d
2025-11-25 tetap dinegosiasikan). `GET /mcp` dengan `Accept: text/event-stream` membuka stream keep-alive.

Tool (deskripsi dalam bahasa Inggris, lengkap dengan `title`, `annotations`, dan `outputSchema`):
`project_context`, `recall`, `remember`, `who_is`, `timeline`, `principles`, `persona_context`,
`family_tree`, `memory_gaps`, `list_subjects`, `compact`.

## Melihat dan mengoreksi

- `engram recall "partai yang didukung"`; tambah `--as-of 2005-01-01` untuk "versi dirinya" saat itu.
- `engram who "Pakde Karno"`, `engram timeline --entity Karno`, `engram principles`.
- `engram review list` lalu `approve`/`reject`: konflik fakta, entitas kembar, data zona abu-abu.

## Tautan antar proyek & graf proyek

Proyek bisa saling ditautkan dengan sintaks Obsidian `[[Nama-Proyek]]` (juga `[[Nama|label]]` dan
`[[Nama#bagian]]`):

- **Catatan proyek**: di halaman proyek, *Edit info proyek*, ketik `[[` untuk saran nama proyek.
- **Memori agent**: teks `remember` (atau observasi sesi) yang memuat `[[Nama-Proyek]]` menautkan proyek
  memori itu ke proyek tujuan. Tautan ke proyek itu sendiri diabaikan.

Proyek tujuan menampilkan backlink: *Ditautkan dari* (catatan) dan *Ditautkan di memori* (episode).
Halaman **Graf proyek** (menu Kerja) dan tab **Graf** di tiap proyek menggambarkan semuanya:

| Garis | Arti |
|---|---|
| panah penuh | tautan `[[...]]` dari catatan atau memori |
| panah putus-putus | sebutan otomatis: memori proyek A menyebut entitas proyek B |
| titik-titik | tag grup yang sama (simpul `#tag`) |
| garis tipis ke simpul ungu | tool, organisasi, orang, atau tempat yang dipakai dua proyek atau lebih |

Simpul berongga adalah proyek yang baru dikenal dari tautan (belum punya memori).

**Tampilan bola 3D (bawaan) atau 2D.** Tombol *3D / 2D* di toolbar berganti tampilan; pilihan diingat.

| Aksi | 3D (bola) | 2D (datar) |
|---|---|---|
| Seret latar | memutar bola | menggeser |
| Shift+seret, tombol tengah, dua jari | menggeser | menggeser |
| Roda, cubit, tombol +/- | zoom | zoom |
| Seret simpul | merapikan di permukaan bola | merapikan |
| Klik simpul | pilih, bola berputar ke simpul itu, panel detail terbuka | pilih, panel detail terbuka |
| Klik ganda simpul / Enter | buka proyek, tag, atau entitas | sama |

Graf global berputar pelan sampai disentuh (tombol putar atau tombol `r` menyalakan/mematikannya;
dimatikan otomatis bila sistem meminta gerak dikurangi). Arahkan kursor ke garis untuk melihat alasan
tautannya, misalnya "Disebut di 3 memori".

**Panel detail** di samping kanvas menampilkan jenis simpul, jumlah memori, kapan terakhir aktif, tag,
dan tetangga per jenis tautan beserta alasannya. Klik tetangga untuk memutar bola ke sana. Tombolnya:
*Buka proyek*, *Fokus di sini* (ganti ke graf lokal proyek itu tanpa pindah halaman; tombol Back browser
kembali ke graf sebelumnya), dan *Salin nama*.

**Warna & filter.** *Warna*: jenis simpul, *tag grup* (seperti "groups" Obsidian; klik legenda untuk
menyorot satu kelompok), atau *aktivitas* (makin redup makin lama tidak aktif). *Aktif dalam* 7/30/90 hari
menyembunyikan proyek yang lama tidak punya memori baru. Tombol jenis tautan dan *Proyek tanpa tautan*
tetap ada.

**Keyboard** (fokus di kanvas): panah = pindah antar simpul (urut jumlah tetangga), Enter = buka,
Esc = tutup panel, `/` = cari, `m` = 3D/2D, `r` = putar otomatis, `+`/`-`/`0` = zoom/muat semua.

**Tautan langsung**: `#graf` untuk graf global, `#graf/<nama-proyek>?depth=2` untuk graf lokal. Di halaman
proyek ada tautan *Buka graf penuh*. Tab Graf proyek menampilkan graf lokal (kedalaman 1 sampai 3) beserta
berkas dan komponen proyek itu. Daftar teks di bawah kanvas memuat tautan yang sama. API:
`GET /api/subjects/{s}/projects/graph?project=<nama>&depth=1`; tiap simpul membawa `last_at` (memori
terakhir). `project_context` menyebut proyek yang tertaut dan proyek yang saling menyebut.

## Grup proyek (konteks lintas repo)

Proyek yang memakai tag grup yang sama (mis. `#VOLTUNES` di *Edit info proyek*) membentuk satu grup.
Saat agent memanggil `project_context` untuk satu repo, hasilnya menyertakan bagian **Ekosistem grup**:

- **Arsitektur bersama** dari proyek payung, yaitu proyek yang namanya sama dengan tag (mis. `VOLTUNES`).
- **Peta repo dalam grup**: tiap repo saudara dengan peran singkat dan tech stack terkininya.
  Bila `task` diisi, repo yang paling relevan dengan tugas tampil lebih dulu.
- **Konvensi & jebakan lintas repo** dan **sesi terbaru di grup** (ambil detailnya dengan `get_episodes`).

Keanggotaan grup dikenali dari tag proyek, dari nama proyek yang sama dengan tag (proyek payung),
dari prefiks `GRUP/repo`, atau dari nama folder induk di path sesi (repo baru di
`.../VOLTUNES/BACKEND/api-baru` langsung masuk grup `VOLTUNES` walau belum diberi tag).
`project_context(project="VOLTUNES")` memberi gambaran seluruh grup.

`recall` dengan `project` juga ikut mencari di repo saudara, dengan proyek sendiri tetap diutamakan.
Matikan dengan argumen `group: false` di kedua tool. Tanpa `budget` eksplisit, bagian grup mendapat
tambahan anggaran `retrieval.group_budget_tokens` (bawaan 3000); dengan `budget` eksplisit, grup
memakai sekitar 40% darinya.

## Pengaturan, tentang & update

Klik ikon roda gigi di kanan atas UI:

- **Umum**: tema (ikuti sistem/terang/gelap), subjek default, dan (desktop) tombol buka folder
  data, inbox, dan konfigurasi.
- **Data & Sync**: lokasi data dan sinkronisasi Google Drive (lihat di bawah).
- **Agent AI**: pasang server MCP ke Antigravity, VS Code, Claude Code, Cursor, Codex CLI, atau Windsurf (lencana *profil coding* menandai client yang hanya memuat tool sesi koding).
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

## Memori tim lewat hub

Siapa pun yang punya akses ke sebuah repo dan memakai engram otomatis berbagi memori proyek itu:
konvensi & jebakan, keputusan, fakta arsitektur, dan judul + ringkasan episode. Memori disimpan di
**hub tim**, bukan di git. Di repo hanya ada satu berkas kecil, `.engram/repo.toml`, yang berubah
hanya saat join key dirotasi.

**1. Menjalankan hub (sekali per tim).** Hub adalah `engram serve --hub` biasa. Cara termudah:
[docker/docker-compose.yml](../docker/docker-compose.yml) yang membangun image dari source dan bergabung ke network Docker nginx Anda, plus contoh blok server [docker/nginx-engram.conf](../docker/nginx-engram.conf).

1. Buat OAuth app:
   - Google Cloud Console, *Credentials*, *OAuth client ID* (Web), redirect
     `https://<domain>/hub/auth/callback/google`.
   - GitHub, *Settings*, *Developer settings*, *OAuth Apps*, callback
     `https://<domain>/hub/auth/callback/github`.
2. Salin `docker/hub.env.example` ke `docker/hub.env`, isi domain, `ENGRAM_HUB_JWT_SECRET`
   (`openssl rand -hex 32`), client id dan secret.
3. Isi `NGINX_NETWORK` dengan network container nginx (`docker network ls`), lalu
   `docker compose -f docker/docker-compose.yml --env-file docker/hub.env up -d --build`.
4. Salin `docker/nginx-engram.conf` ke conf.d nginx, ganti `server_name` dan path sertifikat,
   lalu `nginx -t && nginx -s reload`.

Halaman web hub ada di `https://<domain>/hub`: daftar repo, anggota & peran, aktivitas, pengaturan,
rotasi join key, dan personal access token.

**2. Menghubungkan repo (sekali per repo, oleh satu orang):**

```sh
engram login --hub https://<domain>   # browser terbuka, pilih Google atau GitHub
cd /path/ke/repo
engram repo push --dry-run            # (opsional) lihat dulu apa yang akan dibagikan
engram repo init                      # buat repo di hub, tulis .engram/repo.toml, kirim memori
git add .engram/repo.toml && git commit -m "engram: hubungkan memori tim ke hub" && git push
```

Di UI lokal atau aplikasi desktop: halaman proyek, panel **Memori tim**, login lalu
*Hubungkan ke hub tim*.

**3. Anggota lain** cukup `git pull` dan `engram login` sekali. Sesi agent berikutnya di repo itu
otomatis bergabung (join key dari `repo.toml`), menarik memori rekan, dan di akhir sesi mengirim
memori proyeknya sendiri. Manual: `engram repo join`, `engram repo pull`, `engram repo push`.

**Cara kerjanya:**

- Database tetap SurrealDB embedded di vault masing-masing; embedding record rekan dihitung lokal.
- Hub menyimpan record per (repo, anggota, perangkat) dengan change log `seq`. Push hanya mengirim
  record yang isinya berubah; pull menarik sejak cursor terakhir. Pencabutan (record yang hilang di
  vault penulis, atau anggota yang keluar) mengalir sebagai perubahan: penerima mengkarantina
  episodenya dan menutup faktanya dengan `valid_to`.
- ID record diturunkan dari isinya (kunci judul pelajaran, situasi + keputusan, entitas + predikat
  + objek fakta), sehingga kompaksi atau rekonsiliasi lokal tidak membuat record baru.
- Keputusan rekan menjadi episode keputusan, faktanya masuk lewat rekonsiliasi biasa, pelajaran yang
  sama dengan milik sendiri dihitung sebagai penguatan. Memori hasil impor tidak dikirim ulang.
- Pemicu otomatis: awal sesi agent (pull), akhir sesi (push), dan pemeriksaan tiap 15 menit. Hub
  tidak terjangkau = job diulang dengan backoff; belum login = dilewati dengan catatan di status.

**Akses & keamanan.**

- Peran: owner, maintainer (atur anggota, rotasi key, pengaturan), member (baca + kirim), reader.
- `repo.toml` di repo = siapa pun yang bisa clone boleh bergabung. Untuk repo GitHub privat,
  maintainer bisa menyalakan *Wajibkan pemeriksaan izin GitHub*; anggota lalu bergabung dengan
  `engram repo join --github` (token `gh auth token` dipakai sekali, tidak disimpan hub).
- Join key, refresh token, dan PAT disimpan di hub sebagai hash. Kredensial login lokal ada di
  folder data engram (`hub.json`, mode 0600), dipakai bersama CLI, server lokal, dan aplikasi desktop.
- Akun Google dan GitHub dengan email terverifikasi yang sama otomatis menjadi satu akun.
- Hub tidak pernah menyimpan transkrip mentah, teks episode, embedding, atau daftar berkas.

**Pindah dari bundel git lama (`.engram/members/`):**

```sh
engram login --hub https://<domain>
engram repo migrate                   # kirim semua bundel anggota ke hub, hapus members/, cabut hook
git rm -r --cached --ignore-unmatch .engram && git add .engram/repo.toml
git commit -m "engram: pindahkan memori tim ke hub"
```

Selama masa transisi engram masih membaca bundel lama rekan yang belum migrasi (tanpa menulis ke
repo). Matikan dengan `legacy_import = false`.

**Pengaturan lokal** (`memory.toml`):

```toml
[repo_share]
enabled = true          # false = tanpa pull/push otomatis (perintah manual tetap jalan)
auto_import = true      # tarik memori rekan saat sesi dimulai dan berkala
auto_export = true      # kirim memori perangkat ini saat sesi berakhir dan berkala
interval_minutes = 15
hub_url = "https://<domain>"   # hub bawaan untuk login & init
share_episodes = true   # sertakan judul + ringkasan episode proyek
legacy_import = true    # baca bundel .engram/members/ lama
```

**CI dan agent cloud.** Buat personal access token di halaman *Token akses* hub, lalu set
`ENGRAM_HUB_URL` dan `ENGRAM_HUB_TOKEN`; `engram repo pull` tidak memerlukan browser.

## Kedaulatan data

- `engram export` membuat JSONL + Markdown yang bisa dibaca tanpa software ini.
- `engram backup backup.tar.gz --recipient age1...` membuat backup terenkripsi.
- `engram restore backup.tar.gz.age --identity kunci.txt` memulihkan ke vault baru.
- Ganti model embedding: ubah `[embedding]`, lalu `engram reembed`.
- `engram bench --episodes 100000` mengukur kecepatan sebelum data membesar.

## Mengukur kualitas penyaringan

`engram bench eval` memeriksa apakah potongan chat yang penting benar-benar jadi
episode dan basa-basi terbuang. Vault asli tidak disentuh (evaluasi memakai
database sementara di memori).

1. Buat folder kasus, mis. `eval/wa-keluarga/` (folder `/eval` sudah di-`.gitignore`
   karena berisi chat asli), berisi `source.txt` (ekspor chat) dan `gold.toml`:

   ```toml
   kind = "chat"
   subject_name = "Nama Subjek"

   [[important]]          # harus jadi episode
   quote = "akhirnya aku memutuskan pindah ke Jogja"
   [[noise]]              # harus terbuang
   quote = "oke siap otw"
   ```

   Atau buat langsung dari keputusan review yang sudah ada:
   `engram bench export-gold --out eval/dari-review` (hentikan aplikasi desktop/`serve` dulu).

2. `engram bench eval eval/` menampilkan recall (penting yang jadi episode),
   +review (termasuk zona abu-abu), bocor (noise yang jadi episode), dan daftar
   item yang salah jalur beserta skornya.
3. `engram bench tune eval/` mencoba ratusan kombinasi ambang `[score]` tanpa
   memanggil LLM lagi (hasil segmentasi di-cache di `<kasus>/.cache/`) dan
   mengusulkan nilai untuk `memory.toml`. Atur dengan `--max-leak 0.1` dan
   `--grid "promote=0.40..0.70:0.05,gray=0.30..0.55:0.05"`.

Catatan: setiap segmen dinilai seolah vault kosong (tanpa bonus kebaruan atau
sebutan orang yang sudah dikenal), jadi angkanya sedikit lebih ketat daripada
saat import sungguhan. Pakai `--refresh` setelah mengganti model atau prompt
segmentasi. Usulan tuning dari label yang sedikit (< 40) mudah overfit.


## Sesi kerja agent, privasi, dan pengambilan bertahap

**Apa yang direkam.** Setelah `scripts/install-mcp.sh --client claude` (dengan binary `engram` terpasang),
setiap giliran kerja di Claude Code tersimpan: prompt Anda, ringkasan tiap pemanggilan tool (berkas yang
dibaca/diubah, perintah, hasil singkat), dan jawaban akhir agent. Saat giliran selesai, pipeline mengubahnya
menjadi observasi bertipe (keputusan, bugfix, temuan, fitur, refactor, perubahan), satu ringkasan sesi, dan
pelajaran proyek (konvensi, koreksi Anda, jebakan). I/O tool mentah tetap ada di `vault/sessions/<sesi>/`
agar setiap memori bisa ditelusuri ke sumbernya. Giliran basa-basi dilewati oleh LLM (`skip`).

**Apa yang disuntik saat sesi mulai.** Hook `SessionStart` menambahkan blok ≤ 9.500 karakter berisi
sesi terakhir, konvensi & jebakan proyek, fakta arsitektur yang berlaku, prinsip kerja, dan indeks observasi
terbaru (ID + perkiraan token). Agent lalu memanggil `get_episodes(ids)` hanya untuk yang relevan.
Ukuran blok diukur dan dikurangi otomatis (narasi sesi → prinsip → sesi → fakta → observasi) sampai muat.
Atur di Pengaturan atau `[inject]` di `memory.toml`; `enabled = false` mematikannya.

**Privasi.** Bungkus bagian yang tidak boleh disimpan dengan `<private>…</private>`. Rahasia yang terdeteksi
(AWS, GitHub, OpenAI/Anthropic, Slack, JWT, kunci PEM, webhook, kredensial di URL, dll.) diganti penanda
`<redacted type='…'/>` sebelum disimpan; pola generik (`*_PASSWORD=`, `Authorization: Bearer`) aktif bila
`privacy.redact_generic = true`. Penyaring ini berjalan di satu titik (tahap Ingest) sehingga berlaku untuk
hook, tool `remember`, dan import berkas.

**Bila server mati.** Hook menyimpan event ke `~/Library/Application Support/id.engram.ai/hook-spool/`
(Linux: `~/.local/share/id.engram.ai/`) dan mengirim ulang saat server kembali hidup. Konteks awal sesi
hanya kosong (agent tetap jalan).

**Memeriksa.** Tampilan *Sesi & pelajaran* di UI; `GET /api/subjects/<subjek>/context/inject?project=<nama>`
untuk melihat blok yang akan disuntik; `ENGRAM_HOOK_DEBUG=1` membuat hook mencetak durasi ke stderr.

**Import & berbagi.**
- `engram import-claude-memory` (tanpa `--apply` hanya menampilkan) mengimpor auto-memory Claude Code
  langsung sebagai episode, tanpa LLM, karena isinya sudah terdistilasi.
- Memori proyek bisa dibagikan ke tim lewat repo git: lihat
  [Memori tim lewat hub](#memori-tim-lewat-hub).
