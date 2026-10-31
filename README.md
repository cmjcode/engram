# Platform Memory Ai

## Instalasi

macOS / Linux — memasang CLI `memory` dan aplikasi desktop `mnemonic-memory`:

```sh
curl -fsSL https://raw.githubusercontent.com/cmjcode/mnemonic-memory/main/install.sh | sh
```

Opsi: `--cli-only`, `--desktop-only`, `--version 0.0.2`, `--bin-dir ~/bin`, `--appimage` (Linux), misalnya
`curl -fsSL .../install.sh | sh -s -- --cli-only`. CLI dipasang ke `~/.local/bin`; aplikasi macOS ke `/Applications`,
Linux lewat `.deb`/`.rpm` (atau AppImage bila tanpa sudo). Windows: unduh `.exe`/`.msi` dari
[Releases](https://github.com/cmjcode/mnemonic-memory/releases).

**Update otomatis.** Aplikasi desktop memeriksa rilis baru tak lama setelah dibuka dan setiap 6 jam, atau
lewat menu tray **Periksa update**. Update dipasang setelah disetujui, lalu aplikasi dibuka ulang. Paket update
ditandatangani; kunci privatnya `~/.tauri/mnemonic-memory.key` (dipakai `build.sh`, dan harus disimpan juga
sebagai secret GitHub `TAURI_SIGNING_PRIVATE_KEY`). Manifest `latest.json` disusun oleh
`scripts/updater-manifest.sh`. CLI `memory` belum ikut update otomatis; jalankan ulang `install.sh`.

## Integrasi Agent AI (MCP, Rule & Hook Otomatis)

`mnemonic-memory` dapat dihubungkan ke **Antigravity (IDE & CLI)**, **Gemini CLI**, **Claude Code**, dan **Cursor** sehingga setiap percakapan penting otomatis tersimpan ke memori hidup dan konteks masa lalu dapat diambil kapan saja:

```sh
# Pasang MCP server, Global Rule, dan Hook otomatis ke semua client yang terinstal
./scripts/install-mcp.sh --client all

# Atau pilih client tertentu: antigravity | gemini | claude | cursor
./scripts/install-mcp.sh --client antigravity

# Hapus integrasi dari semua client
./scripts/install-mcp.sh --uninstall --client all
```

Apa saja yang dipasang oleh `scripts/install-mcp.sh`:

1. **Server MCP (`memori`)**: Menyediakan tool `remember`, `recall`, `who_is`, `timeline`, `principles`, `persona_context`, `family_tree`, `memory_gaps`, dan `list_subjects` lewat HTTP (`http://127.0.0.1:7878/mcp`) dengan token tetap `MEMORY_TOKEN` (`LaunchAgent` `id.memorihidup.token`).
2. **Global Rule (`GEMINI.md` & `CLAUDE.md`)**: Menginstruksikan agent di `~/.gemini/GEMINI.md`, `~/.gemini/config/GEMINI.md`, dan `~/.claude/CLAUDE.md` agar proaktif memanggil `remember` saat pengguna menceritakan kisah/fakta/prinsip hidup, serta memanggil `recall`/`who_is`/`timeline`/`principles` saat menjawab pertanyaan tentang masa lalu.
3. **Hook Otomatis (`~/.local/bin/mnemonic-auto-remember`)**:
   * Dipasang pada event `Stop` di Antigravity (`~/.gemini/config/hooks.json`) dan Claude Code (`~/.claude/settings.json`), serta `AfterAgent` di Gemini CLI (`~/.gemini/settings.json`).
   * Setiap giliran percakapan selesai, script `scripts/auto-remember-hook.py` otomatis mengekstrak pesan pengguna & balasan asisten dari transkrip, menyaring duplikat, lalu mengirimnya ke pipeline `remember` (yang akan menyaring obrolan basa-basi lewat konfigurasi `[score]`).
   * **Fallback Offline**: Bila aplikasi desktop / `memory serve` sedang tidak aktif, percakapan otomatis disimpan sebagai berkas `.md` ke `vault/inbox/` untuk diimpor otomatis begitu aplikasi dibuka kembali.

**1\. Kategori "AI Legacy & Biographer" (Fokus ke Konsumen/Keluarga)** Platform ini memang dibuat khusus untuk merekam kisah hidup, suara, dan kenangan seseorang untuk diwariskan ke anak-cucu.

> * **HereAfter AI & StoryFile:** Mengambil pendekatan wawancara interaktif. Pengguna menjawab ratusan pertanyaan tentang hidup mereka (via audio/video). Nantinya, keluarga bisa "mengobrol" dengan rekaman tersebut. *Kelemahannya:* Arsitektur di belakangnya masih sangat kaku (mirip sistem pencocokan Q\&A berbasis RAG sederhana), belum menjadi *cognitive engine* yang benar-benar bisa berpikir dengan pola pikir orang tersebut.  
> * **Eternos.life & Personal.ai:** Lebih maju karena membangun *Personal Language Model* dan *Memory Stack* dari kisah hidup pengguna. *Kelemahannya:* Semuanya berbasis *cloud proprietary* (data hidupmu dikunci di server mereka dengan biaya langganan mahal) dan kurang mendalam dalam memetakan perubahan sifat manusia secara kronologis (*temporal graph*).

**2\. Kategori "Active Lifelogging" (Perekam Aktivitas Real-Time)** Platform yang fokus merekam semua yang dilihat dan didengar pengguna sehari-hari.

> * **Limitless (dulunya Rewind.ai) & Omi (Friend):** Menggunakan perangkat *wearable* (kalung/pendant) dan perekam layar untuk mencatat seluruh rapat, obrolan, dan aktivitas harian ke dalam database vektor lokal/cloud.  
> * *Kelemahannya:* Sistem ini hebat untuk menjawab *"Tadi pagi aku ngobrol apa sama Budi?"*, tapi buruk untuk merangkum masa lalu. Mereka kebanjiran data sampah harian (*noise*) dan tidak punya arsitektur untuk mengekstrak kebijaksanaan (*wisdom*), nilai hidup, atau menyusun cerita masa kecil 30 tahun lalu.

**3\. Kategori "Developer Memory Infrastructure" (Engine di Balik Layar)** Jika melihat dari sisi *engine* pembangunnya (bukan aplikasi jadi untuk konsumen), beberapa *startup* infrastruktur AI sedang berlomba di area ini:

> * **Zep (dengan engine *Graphiti*\-nya) & Cognee:** Dua *framework* ini paling mendekati secara teknis karena sudah menggunakan **Temporal Knowledge Graph**. Mereka bisa melacak kapan sebuah fakta mulai berlaku dan kapan kedaluwarsa.  
> * **Mem0 & Letta (MemGPT):** Sangat kuat di manajemen *state* dan memori jangka panjang untuk *enterprise agent*, tapi didesain untuk asisten kerja/komersial, bukan untuk memodelkan psikologi, emosi, dan fase kehidupan manusia.  
> * **Evermind (EverOS) & Supermemory:** Fokus pada *Memory OS* yang menggabungkan dokumen, chat, dan *multi-modal retrieval* untuk memori jangka panjang.

**Celah Besar yang Belum Terpecahkan (Peluang Kalau Mau Bangun Sendiri)**

Jika kamu ingin membangun ini sendiri, kamu tidak sedang membuat ulang roda—kamu justru bisa mengisi lubang besar yang belum disentuh pemain di atas:

> 1. **Kedaulatan Data (*Self-Hosted & Sovereign* untuk 50+ Tahun):** Kisah hidup dan warisan pemikiran keluarga adalah data paling sensitif. Menyimpannya di *startup SaaS* luar negeri sangat berisiko (kalau *startup*\-nya bangkrut 5 tahun lagi, memori hidup itu hilang). Sistem *local-first* atau *self-hosted* dengan format penyimpanan terbuka adalah keharusan untuk *legacy system*.  
> 2. **Pemodelan Karakter & Prinsip Hidup (*Wisdom Layer*):** Platform yang ada sekarang baru sebatas menyimpan **Fakta** (*"Dia pernah merintis usaha di Jogja tahun 2015"*). Belum ada yang secara khusus mengekstrak **Pola Keputusan/Prinsip** (*"Setiap kali menghadapi krisis bisnis, orang ini selalu memprioritaskan kesejahteraan timnya dulu baru menghitung rugi materi, karena dipengaruhi didikan ibunya"*).  
> 3. **Konteks Budaya & Bahasa Lokal:** Orang Indonesia bercerita dengan struktur yang unik—banyak kiasan, percampuran bahasa daerah (Jawa/Sunda/dll), dan struktur silsilah keluarga besar yang rumit. Platform Barat sering gagal menangkap nuansa emosional dan relasi kekerabatan ini.


