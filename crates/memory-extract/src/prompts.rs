//! Prompt sistem (bahasa Indonesia). Dipusatkan di sini agar mudah dievaluasi
//! dan disetel tanpa menyentuh logika pipeline.

pub const SEGMENT_SYSTEM: &str = "\
Kamu memecah transkrip/catatan kisah hidup menjadi segmen cerita yang utuh secara topik.
Input berupa unit bernomor [i]. Kelompokkan unit yang berurutan menjadi segmen.
Untuk setiap segmen isi:
- start, end: indeks unit pertama dan terakhir (inklusif), berurutan, tidak tumpang tindih, mencakup semua unit.
- topic: topik singkat.
- about_subject: true bila tentang hidup, keluarga, pengalaman, atau cara berpikir SUBJEK; false untuk iklan, berita yang dibacakan, obrolan orang lain yang tidak terkait, atau rekaman latar.
- speaker: siapa yang bercerita bila jelas.
- significance 0..1: 0 = basa-basi, salam, logistik, 'oke/sip'; 0.3 = rutinitas; 0.6 = pengalaman bermakna; 1 = peristiwa hidup besar, keputusan penting, nilai hidup.
Pahami campuran bahasa Indonesia, Jawa, Sunda, dan kiasan. Keluarkan JSON saja.";

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
Kamu adalah pengekstrak memori biografis. Ubah satu segmen cerita menjadi memori terstruktur.
ATURAN WAJIB:
1. Hanya ekstrak yang tertulis atau tersirat jelas di teks. Jangan mengarang.
2. Setiap fakta, relasi kekerabatan, keputusan, dan sinyal sifat WAJIB punya `quote` yang disalin PERSIS dari teks (bahasa asli, tanpa diterjemahkan).
3. 'saya/aku/abdi/kula/dalem/kami(=keluarga subjek)' merujuk SUBJEK: entitas dengan is_self=true dan name = nama subjek.
4. Waktu (`time` episode dan `time` tiap fakta) pilih kind:
   exact (year+month+day) | approx ('sekitar 2015' -> year=2015) | range (year..year_end, ongoing bila masih) |
   age ('umur 20' -> age=20; 'umur 20-an' -> age=20, age_end=29) | relative ('setahun setelah nikah' -> anchor='nikah', offset_years=1) |
   life_phase ('waktu SD' -> phase='SD', 'masa merantau' -> phase='merantau') | current ('sekarang', 'saat ini') |
   past ('dulu', 'pernah', 'mbiyen', 'baheula' tanpa tahun) | unknown.
   Isi `text` dengan ungkapan waktu aslinya. JANGAN menebak tahun yang tidak disebut.
5. Fakta: subject = nama entitas pelaku, predicate = kata kerja singkat (mendukung, tinggal di, bekerja di, lahir di, menikah dengan, mendirikan),
   object = objek, object_is_entity = true bila objeknya orang/tempat/organisasi.
   category snake_case stabil: afiliasi_politik, tempat_tinggal, pekerjaan, pendidikan, status_pernikahan, agama, usaha, kesehatan, hobi, kepemilikan, peristiwa.
   kind = 'preference' untuk pilihan/afiliasi/pandangan (partai, tim, agama, selera), selain itu 'fact'.
   exclusive = true bila hanya satu nilai bisa berlaku pada satu waktu (partai politik, tempat tinggal utama, pekerjaan utama, status pernikahan, agama).
   confidence 0..1.
6. Emosi sesaat (sedih, marah, bangga saat itu) -> emotions. Sifat atau nilai yang TERLIHAT dari tindakan berulang/tegas -> trait_signals
   (kind 'trait' untuk sifat, 'value' untuk nilai hidup; polarity -1 bila cerita menunjukkan sifat itu sudah ditinggalkan). Emosi BUKAN sifat.
7. Kekerabatan: person adalah `term` dari `of` (kosongkan `of` bila terhadap subjek). Tulis term apa adanya (pakde, bulik, mbah kakung, uwa, teteh, besan, ipar).
8. decisions: situasi -> tindakan -> alasan, bila subjek mengambil keputusan.
9. quotes: kalimat khas subjek (pepatah, nasihat, ungkapan) yang layak dikenang.
10. title singkat; summary 1-3 kalimat bahasa Indonesia yang menerjemahkan kiasan/bahasa daerah.
Keluarkan JSON saja.";

pub fn extract_user(subject_name: &str, observed: &str, birth: Option<&str>, known_entities: &str, text: &str) -> String {
    format!(
        "SUBJEK: {subject_name}\nTanggal cerita direkam: {observed}\nTanggal lahir subjek: {}\nEntitas yang sudah dikenal (pakai nama yang sama bila merujuk orang yang sama):\n{}\n\nTEKS:\n{text}",
        birth.unwrap_or("tidak diketahui"),
        if known_entities.is_empty() { "-" } else { known_entities }
    )
}

pub const SAME_ENTITY_SYSTEM: &str = "\
Tentukan apakah dua sebutan merujuk orang/tempat/organisasi yang SAMA dalam kisah hidup seorang subjek.
Perhatikan panggilan kekerabatan (Mbah, Pakde, Bulik), nama kecil, gelar, dan nama Jawa/Sunda.
Jika ragu, jawab same=false. Keluarkan JSON {same, confidence, reason}.";

pub const PRINCIPLE_SYSTEM: &str = "\
Kamu menyimpulkan PRINSIP atau POLA KEPUTUSAN seseorang dari beberapa keputusan nyata di hidupnya.
Prinsip harus: (1) konsisten di SEMUA bukti yang kamu kutip, (2) berupa pola perilaku/keputusan, bukan fakta,
(3) ditulis sebagai kalimat orang ketiga, mis. 'Saat usaha sedang krisis, ia mendahulukan gaji karyawan sebelum menghitung kerugiannya sendiri.'
Isi origin bila bukti menyebut asal pengaruhnya (mis. 'didikan ibunya').
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
