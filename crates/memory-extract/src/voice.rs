//! Profil gaya tutur (Lapisan 4): idiom, campuran bahasa, contoh kutipan,
//! dan deskripsi gaya untuk mode persona.

use std::collections::{BTreeMap, HashMap, HashSet};

use memory_core::ids::now_rfc3339;
use memory_core::model::{EpisodeStatus, VoiceProfile};
use memory_core::text::tokens;
use memory_llm::{ModelRole, complete_json};
use memory_store::EpisodeFilter;

use crate::llm_types::TextOut;
use crate::{Pipeline, Result, prompts};

const STOP: &[&str] = &[
    "yang", "dan", "di", "ke", "dari", "itu", "ini", "saya", "aku", "kita", "kami", "dia", "ada", "tidak", "untuk", "dengan", "juga",
    "sudah", "akan", "karena", "kalau", "tapi", "atau", "pada", "jadi", "ya", "the", "a", "to", "of",
];

/// N-gram (2..=4 kata) yang muncul di >= 2 kutipan berbeda, tidak seluruhnya stopword.
pub fn idioms(quotes: &[String], top: usize) -> Vec<(String, u32)> {
    let mut seen: HashMap<String, HashSet<usize>> = HashMap::new();
    for (qi, q) in quotes.iter().enumerate() {
        let t = tokens(q);
        for n in 2..=4 {
            for w in t.windows(n) {
                if w.iter().all(|x| STOP.contains(&x.as_str())) {
                    continue;
                }
                seen.entry(w.join(" ")).or_default().insert(qi);
            }
        }
    }
    let mut v: Vec<(String, u32)> = seen.into_iter().filter(|(_, s)| s.len() >= 2).map(|(k, s)| (k, s.len() as u32)).collect();
    // Frekuensi dulu, lalu frasa lebih panjang.
    v.sort_by(|a, b| b.1.cmp(&a.1).then(b.0.split(' ').count().cmp(&a.0.split(' ').count())).then(a.0.cmp(&b.0)));
    let mut out: Vec<(String, u32)> = Vec::new();
    for (k, c) in v {
        if out.iter().any(|(o, oc)| *oc == c && o.contains(&k)) {
            continue;
        }
        out.push((k, c));
        if out.len() >= top {
            break;
        }
    }
    out
}

impl Pipeline {
    pub async fn build_voice_profile(&self, subject: &str) -> Result<VoiceProfile> {
        let mut eps = self
            .store
            .list_episodes(subject, &EpisodeFilter { status: Some(EpisodeStatus::Active), limit: 20_000, ..Default::default() })
            .await?;
        eps.sort_by(|a, b| b.importance.total_cmp(&a.importance));
        let quotes: Vec<String> = eps.iter().flat_map(|e| e.quotes.iter().cloned()).collect();
        let mut lang_counts: BTreeMap<String, f32> = BTreeMap::new();
        for e in &eps {
            for l in &e.languages {
                *lang_counts.entry(l.clone()).or_default() += 1.0;
            }
        }
        let total: f32 = lang_counts.values().sum::<f32>().max(1.0);
        let language_mix = lang_counts.into_iter().map(|(k, v)| (k, (v / total * 100.0).round() / 100.0)).collect();
        let sample_quotes: Vec<String> = quotes.iter().take(12).cloned().collect();
        let style = if quotes.is_empty() {
            String::new()
        } else {
            let user = quotes.iter().take(30).map(|q| format!("- {q}")).collect::<Vec<_>>().join("\n");
            complete_json::<TextOut>(self.llm.as_ref(), ModelRole::Light, "voice", prompts::VOICE_SYSTEM, &user, 1)
                .await
                .map(|t| t.text)
                .unwrap_or_default()
        };
        let vp = VoiceProfile { subject: subject.to_string(), style, idioms: idioms(&quotes, 20), language_mix, sample_quotes, updated_at: now_rfc3339() };
        self.store.put_voice_profile(&vp).await?;
        Ok(vp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_repeated_sayings() {
        let q = vec![
            "Urip iku urup, Le, harus menyala buat orang lain.".to_string(),
            "Ingat, urip iku urup.".to_string(),
            "Jangan lupa urip iku urup ya".to_string(),
            "Hari ini hujan".to_string(),
        ];
        let id = idioms(&q, 5);
        assert_eq!(id[0], ("urip iku urup".to_string(), 3));
    }
}
