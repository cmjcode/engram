//! Prompt sistem (bahasa Indonesia). Dipusatkan di sini agar mudah dievaluasi
//! dan disetel tanpa menyentuh logika pipeline.

pub const SEGMENT_SYSTEM: &str = "\
Kamu memecah transkrip percakapan AI, catatan kerja/koding, atau kisah hidup menjadi segmen yang utuh secara topik.
Input berupa unit bernomor [i]. Kelompokkan unit yang berurutan menjadi segmen.
Untuk setiap segmen isi:
- start, end: indeks unit pertama dan terakhir (inklusif), berurutan, tidak tumpang tindih, mencakup semua unit.
- topic: topik singkat.
- about_subject: true bila membahas proyek, kode, arsitektur, aturan bisnis, debugging/solusi bug, preferensi kerja/tool, pengetahuan teknis, maupun kehidupan, keluarga, pengalaman, atau cara berpikir SUBJEK; false HANYA untuk iklan, berita luar yang tidak terkait, atau rekaman latar acak.
- speaker: siapa yang berbicara bila jelas.
- significance 0..1: 0 = basa-basi, salam, logistik, 'oke/sip', log kompilasi mentah tanpa kesimpulan; 0.3 = rutinitas biasa; 0.6 = konteks proyek, perbaikan bug, atau pengalaman bermakna; 1 = keputusan arsitektur/teknis penting, aturan bisnis inti, konvensi kerja, peristiwa hidup besar, atau nilai/prinsip.
Pahami istilah teknis pemrograman maupun campuran bahasa Indonesia, Inggris, Jawa, Sunda, dan kiasan. Keluarkan JSON saja.";

pub fn segment_user(subject_name: &str, units: &[(usize, String)], examples: &str) -> String {
    let mut s = format!("SUBJEK: {subject_name}\n");
    if !examples.is_empty() {
        s.push_str("\nKeputusan reviewer sebelumnya (pakai sebagai acuan significance):\n");
        s.push_str(examples);
        s.push('\n');
    }
    s.push_str("\nUNIT:\n");
    for (i, u) in units {
        s.push_str(&format!("[{i}] {u}\n"));
    }
    s
}

pub const EXTRACT_SYSTEM: &str = "\
Kamu adalah pengekstrak memori AI terpadu (mencakup pengetahuan engineering/pemrograman, pekerjaan profesional, dan biografi personal). Ubah satu segmen menjadi memori terstruktur.
ATURAN WAJIB:
1. Hanya ekstrak yang tertulis atau tersirat jelas di teks. Jangan mengarang.
2. Setiap fakta, relasi kekerabatan, keputusan, dan sinyal sifat/preferensi WAJIB punya `quote` yang disalin PERSIS dari teks (bahasa asli, tanpa diterjemahkan).
3. 'saya/aku/abdi/kula/dalem/gue/user/pengguna' merujuk SUBJEK: entitas dengan is_self=true dan name = nama subjek.
4. Entitas (`entities`): pilih `kind` dengan tepat:
   - `project`: repositori, microservice, aplikasi, produk, atau workspace (mis. `BACKEND/api-master-sensor`, `LLM-MEMORY`).
   - `component`: berkas kode, modul, crate/package, endpoint API, tabel DB, atau fungsi (mis. `total_value_formulation_test.go`, `resolve.rs`).
   - `tool`: bahasa pemrograman, framework, library, database, CLI, atau AI tool (mis. `Rust`, `SurrealDB`, `Claude Code`, `Antigravity`).
   - `person`: hanya manusia/orang nyata. JANGAN PERNAH memberi kind `person` untuk nama repo, service, atau berkas!
   - `org`: perusahaan, tim, instansi, atau organisasi.
   - `place`: kota, negara, lokasi fisik.
   - `event` | `object` | `concept`: peristiwa, benda fisik, atau konsep/pola arsitektur.
   PENTING: Jangan menggabungkan nama berkas dan nama repo menjadi satu nama entitas seperti 'file_x.go pada BACKEND/repo_y'. Pisahkan menjadi entitas `component` ('file_x.go') dan entitas `project` ('BACKEND/repo_y'), lalu hubungkan dengan fakta.
5. Waktu (`time` episode dan `time` tiap fakta) pilih kind:
   exact (year+month+day) | approx ('sekitar 2015' -> year=2015) | range (year..year_end, ongoing bila masih) |
   age ('umur 20' -> age=20; 'umur 20-an' -> age=20, age_end=29) | relative ('setahun setelah nikah' -> anchor='nikah', offset_years=1) |
   life_phase ('waktu SD' -> phase='SD', 'masa merantau' -> phase='merantau') | current ('sekarang', 'saat ini') |
   past ('dulu', 'pernah', 'mbiyen', 'baheula' tanpa tahun) | unknown.
   Isi `text` dengan ungkapan waktu aslinya. JANGAN menebak tahun yang tidak disebut.
6. Fakta: subject = nama entitas pelaku (bisa subjek, proyek, atau komponen), predicate = kata kerja singkat (menggunakan, mengembangkan, memiliki komponen, memperbaiki bug, menetapkan aturan, mendukung, tinggal di, bekerja di),
   object = objek, object_is_entity = true bila objeknya entitas (proyek/komponen/tool/orang/tempat/organisasi).
   category snake_case stabil:
   - Teknis & Kerja: arsitektur, tech_stack, konvensi_kode, aturan_bisnis, bug_fix, infrastruktur, workflow, preferensi_tool, proyek, komponen.
   - Personal & Biografi: afiliasi_politik, tempat_tinggal, pekerjaan, pendidikan, status_pernikahan, agama, usaha, kesehatan, hobi, kepemilikan, peristiwa.
   kind = 'preference' untuk pilihan/standar/konvensi/pandangan (pilihan tech stack, gaya koding, preferensi tool, afiliasi), selain itu 'fact'.
   exclusive = true HANYA bila hanya satu nilai bisa berlaku pada satu waktu untuk entitas & kategori tersebut (mis. database utama, tempat tinggal utama, pekerjaan utama, status pernikahan). Untuk daftar fitur/komponen/proyek/hobi isi exclusive = false.
   confidence 0..1.
7. Emosi sesaat -> emotions. Sifat, standar kerja, atau nilai yang TERLIHAT dari tindakan/preferensi tegas -> trait_signals
   (kind 'trait' untuk sifat/kebiasaan kerja, 'value' untuk nilai/prinsip engineering atau hidup; polarity -1 bila sudah ditinggalkan).
8. Kekerabatan: person adalah `term` dari `of` (kosongkan `of` bila terhadap subjek). Tulis term apa adanya (pakde, bulik, mbah kakung, uwa, teteh, besan, ipar). Kosongkan bila teks membahas hal teknis.
9. decisions: situasi/masalah -> tindakan/keputusan teknis atau personal -> alasan (mengapa memilih arsitektur/solusi/keputusan tersebut).
10. quotes: kalimat khas, aturan tegas, atau prinsip penting yang dinyatakan subjek.
11. title singkat; summary 1-3 kalimat bahasa Indonesia yang padat dan jelas.
Keluarkan JSON saja.";

pub fn extract_user(subject_name: &str, observed: &str, birth: Option<&str>, known_entities: &str, text: &str) -> String {
    format!(
        "SUBJEK: {subject_name}\nTanggal direkam: {observed}\nTanggal lahir subjek: {}\nEntitas yang sudah dikenal (pakai nama yang sama HANYA bila benar-benar merujuk entitas yang sama):\n{}\n\nTEKS:\n{text}",
        birth.unwrap_or("tidak diketahui"),
        if known_entities.is_empty() { "-" } else { known_entities }
    )
}

pub const SAME_ENTITY_SYSTEM: &str = "\
Tentukan apakah dua sebutan merujuk entitas (orang/proyek/komponen/tool/organisasi/tempat) yang SAMA.
ATURAN KETAT:
1. Dua repositori, microservice, modul, endpoint, atau berkas kode dengan nama berbeda (meskipun memiliki prefix yang sama seperti 'BACKEND/api-master-sensor' dan 'BACKEND/api-master-product') adalah BERBEDA -> wajib jawab same=false, confidence=1.0.
2. Sebuah berkas atau komponen di dalam suatu proyek (misal 'berkas autentikasi pada repo X' vs 'repo X') adalah bagian dari proyek tersebut, BUKAN entitas yang sama -> wajib jawab same=false, confidence=1.0.
3. Proyek/komponen hanya dianggap sama bila satu sebutan adalah nama pendek/alias persis dari yang lain (mis. 'api-master-sensor' dan 'BACKEND/api-master-sensor').
4. Untuk orang (Person), perhatikan panggilan kekerabatan (Mbah, Pakde, Bulik), nama kecil, gelar, dan nama Jawa/Sunda.
Jika ragu, jawab same=false. Keluarkan JSON {same, confidence, reason}.";

pub const PRINCIPLE_SYSTEM: &str = "\
Kamu menyimpulkan PRINSIP KERJA, STANDAR TEKNIS/ARSITEKTUR, atau POLA KEPUTUSAN seseorang dari beberapa keputusan nyata.
Prinsip harus: (1) konsisten di SEMUA bukti yang kamu kutip, (2) berupa pola keputusan/standar engineering/cara kerja/prinsip hidup, bukan sekadar fakta tunggal,
(3) ditulis sebagai kalimat orang ketiga yang jelas, mis. 'Dalam pengembangan backend, ia selalu memisahkan validasi aturan bisnis dari layer handler dan mengutamakan pengujian deterministik.' atau 'Saat usaha sedang krisis, ia mendahulukan gaji karyawan sebelum menghitung kerugiannya sendiri.'
Isi domain (mis. 'engineering', 'arsitektur', 'workflow', 'bisnis', 'keluarga') dan isi origin bila bukti menyebut asal pengaruhnya.
Bila bukti tidak menunjukkan pola yang konsisten, kosongkan statement.
evidence_ids hanya berisi ID episode dari input. Keluarkan JSON saja.";

pub const VERIFY_SYSTEM: &str = "\
Kamu pemeriksa yang skeptis. Diberikan sebuah PERNYATAAN tentang seseorang dan beberapa EPISODE (teks asli).
Untuk tiap episode, nilai apakah teksnya benar-benar mendukung pernyataan itu (bukan sekadar topiknya mirip).
supported_ids = ID episode yang mendukung. confidence 0..1 untuk pernyataan secara keseluruhan; rendah bila ada episode yang bertentangan.
Jangan memakai pengetahuan di luar teks. Keluarkan JSON saja.";

pub const DISPOSITION_SYSTEM: &str = "\
Kamu menulis deskripsi singkat (1-2 kalimat, bahasa Indonesia) tentang sebuah sifat atau nilai hidup seseorang,
hanya berdasarkan kutipan bukti yang diberikan. Sebut perubahan bila bukti menunjukkan sifat itu berubah seiring waktu.
Keluarkan JSON {text}.";

pub const LIFE_PHASE_SYSTEM: &str = "\
Tulis ringkasan naratif (maks 150 kata, bahasa Indonesia) tentang satu fase hidup seseorang dari daftar ringkasan episode.
Urutkan kronologis, sebut orang dan tempat penting, jangan menambah fakta yang tidak ada. Keluarkan JSON {text}.";

pub const VOICE_SYSTEM: &str = "\
Dari kutipan-kutipan asli berikut, deskripsikan GAYA BERTUTUR orang ini dalam 3-5 kalimat bahasa Indonesia:
pilihan kata, campuran bahasa daerah, ungkapan/pepatah favorit, nada (humor, tegas, lembut), cara membuka/menutup cerita.
Jangan menilai isi ceritanya. Keluarkan JSON {text}.";

pub const INTERVIEW_SYSTEM: &str = "\
Kamu pewawancara biografi yang hangat dan sopan (gaya bahasa Indonesia sehari-hari, boleh menyapa 'Bapak/Ibu').
Buat SATU pertanyaan terbuka yang memancing cerita konkret (siapa, di mana, apa yang dirasakan, apa yang diputuskan),
untuk mengisi celah memori yang dijelaskan. Jangan bertanya ya/tidak. Keluarkan JSON {text}.";

pub const COMPACT_EPISODES_SYSTEM: &str = "\
Kamu merangkum kumpulan EPISODE memori yang saling berkaitan (misal catatan kerja/debugging pada topik atau proyek yang sama) menjadi satu atau beberapa episode kanonik yang padat tanpa mengubah atau menghilangkan isi intinya.
ATURAN WAJIB:
1. Hanya gabungkan episode (minimal 2 episode per grup) yang benar-benar membahas topik, peristiwa, atau rangkaian pekerjaan yang sama. Episode yang topiknya berdiri sendiri JANGAN dimasukkan ke rollups.
2. Setiap ID episode hanya boleh muncul di maksimal satu grup `episode_ids`.
3. `title`: judul singkat dan jelas (bahasa Indonesia).
4. `summary`: ringkasan padat 1-3 kalimat (bahasa Indonesia).
5. `text`: gabungan naratif yang utuh dan padat dalam bahasa Indonesia, WAJIB mempertahankan seluruh fakta inti, nama orang/proyek/entitas, angka/tanggal, serta keputusan penting dari semua episode anggota. Jangan mengarang hal di luar teks.
Keluarkan JSON {rollups: [{episode_ids, title, summary, text}]}.";

pub const COMPACT_FACTS_SYSTEM: &str = "\
Kamu merapikan daftar FAKTA aktif milik satu entitas agar tidak ada duplikasi atau log status sesaat, tanpa menghilangkan informasi penting.
ATURAN WAJIB:
1. `merges`: gabungkan fakta-fakta yang maknanya SAMA atau tumpang tindih (misal 'mengembangkan project X' dan 'memiliki proyek X'). Pilih satu `keep_id` terbaik, masukkan sisanya ke `absorb_ids`. Opsional perbaiki `predicate` dan `object_text` agar lebih bersih.
2. `closures`: daftar `fact_id` yang dulunya benar tetapi sudah selesai/digantikan oleh fakta lain yang lebih baru (akan diberi tanggal berakhir `valid_to` sebagai riwayat).
3. `rejections`: daftar `fact_id` yang hanya berupa aktivitas/proses teknis sesaat (misal 'sedang menjalankan kompilasi', 'meminta perbaikan bug hari ini') dan BUKAN fakta biografis/arsitektur jangka panjang.
4. Jangan memasukkan ID yang sama ke lebih dari satu aksi. Fakta yang sudah bersih dan masih berlaku biarkan saja (tidak perlu disebut).
Keluarkan JSON {merges, closures, rejections}.";
