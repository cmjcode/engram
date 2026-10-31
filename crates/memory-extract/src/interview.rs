//! Wawancara terpandu: pertanyaan berikutnya dipilih dari CELAH memori
//! (fase hidup yang episodenya sedikit, orang yang baru disebut sekali),
//! bukan dari daftar pertanyaan statis.

use serde::{Deserialize, Serialize};

use memory_core::model::{EntityKind, EntityStatus, EpisodeStatus, SourceKind};
use memory_core::text::truncate_chars;
use memory_core::time::STANDARD_PHASES;
use memory_llm::{ModelRole, complete_json};
use memory_store::EpisodeFilter;

use crate::ingest::{IngestDoc, IngestOutcome};
use crate::llm_types::TextOut;
use crate::{Pipeline, Result, prompts};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InterviewQuestion {
    pub question: String,
    /// phase | entity | general
    pub gap: String,
    pub target: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Gap {
    pub gap: String,
    pub target: Option<String>,
    pub description: String,
    pub priority: u32,
}

fn template(gap: &Gap) -> String {
    match (gap.gap.as_str(), gap.target.as_deref()) {
        ("phase", Some("masa kecil")) => "Ceritakan rumah tempat Bapak/Ibu tumbuh waktu kecil: siapa saja yang tinggal di sana, dan kejadian apa yang paling membekas?".into(),
        ("phase", Some("remaja")) => "Waktu remaja, siapa teman atau guru yang paling berpengaruh, dan apa yang Bapak/Ibu pelajari darinya?".into(),
        ("phase", Some("dewasa muda")) => "Bagaimana awal Bapak/Ibu mulai bekerja atau merantau? Keputusan apa yang paling berat waktu itu?".into(),
        ("phase", Some("dewasa")) => "Ceritakan masa membangun keluarga dan pekerjaan: tantangan terbesar apa yang dihadapi, dan bagaimana menyelesaikannya?".into(),
        ("phase", Some("paruh baya")) => "Di usia paruh baya, apa yang berubah dalam cara Bapak/Ibu memandang hidup dan keluarga?".into(),
        ("phase", Some(p)) => format!("Ceritakan lebih banyak tentang masa {p}: apa yang terjadi, siapa yang terlibat, dan apa yang Bapak/Ibu rasakan?"),
        ("entity", Some(name)) => format!("Tadi sempat disebut {name}. Siapa beliau, dan cerita apa yang paling diingat bersama beliau?"),
        _ => "Nasihat apa yang paling ingin Bapak/Ibu wariskan ke anak-cucu, dan dari pengalaman apa nasihat itu lahir?".into(),
    }
}

impl Pipeline {
    /// Daftar celah memori, prioritas tertinggi dulu.
    pub async fn memory_gaps(&self, subject: &str) -> Result<Vec<Gap>> {
        let subj = self.ensure_subject(subject, None).await?;
        let eps = self
            .store
            .list_episodes(subject, &EpisodeFilter { status: Some(EpisodeStatus::Active), limit: 20_000, ..Default::default() })
            .await?;
        let current_age = subj.anchors.age_at(chrono::Utc::now().date_naive());
        let mut gaps = Vec::new();
        for (name, from, _) in STANDARD_PHASES {
            if current_age.is_some_and(|a| a < *from) {
                continue;
            }
            let n = eps.iter().filter(|e| e.life_phase.as_deref() == Some(*name)).count();
            if n < 10 {
                gaps.push(Gap {
                    gap: "phase".into(),
                    target: Some((*name).to_string()),
                    description: format!("Fase hidup '{name}' baru punya {n} cerita."),
                    priority: if n < 3 { 100 - n as u32 } else { 50 - n as u32 },
                });
            }
        }
        for e in self.store.list_entities(subject, Some(EntityKind::Person), 500, 0).await? {
            if e.status == EntityStatus::Provisional && e.mention_count <= 1 && !e.is_self {
                gaps.push(Gap {
                    gap: "entity".into(),
                    target: Some(e.name.clone()),
                    description: format!("'{}' baru disebut sekali; belum jelas siapa dan apa perannya.", e.name),
                    priority: 70,
                });
            }
        }
        if gaps.is_empty() {
            gaps.push(Gap { gap: "general".into(), target: None, description: "Memori cukup lengkap; gali nilai dan nasihat hidup.".into(), priority: 10 });
        }
        gaps.sort_by(|a, b| b.priority.cmp(&a.priority).then(a.target.cmp(&b.target)));
        Ok(gaps)
    }

    pub async fn next_question(&self, subject: &str) -> Result<InterviewQuestion> {
        let subj = self.ensure_subject(subject, None).await?;
        let gaps = self.memory_gaps(subject).await?;
        let gap = gaps.into_iter().next().expect("selalu ada minimal satu celah");
        let user = format!("Nama narasumber: {}\nCelah memori: {}", subj.name, gap.description);
        let question = match complete_json::<TextOut>(self.llm.as_ref(), ModelRole::Light, "interview", prompts::INTERVIEW_SYSTEM, &user, 1).await {
            Ok(t) if !t.text.trim().is_empty() => t.text.trim().to_string(),
            _ => template(&gap),
        };
        Ok(InterviewQuestion { question, gap: gap.gap, target: gap.target, reason: gap.description })
    }

    /// Simpan jawaban wawancara sebagai sumber baru (lalu diproses pipeline).
    pub async fn answer_question(&self, subject: &str, question: &str, answer: &str) -> Result<IngestOutcome> {
        self.ingest(IngestDoc {
            subject: subject.to_string(),
            kind: SourceKind::Interview,
            title: format!("Wawancara: {}", truncate_chars(question, 80)),
            text: format!("Pertanyaan: {question}\nJawaban: {answer}"),
            meta: serde_json::json!({"question": question}),
            ..Default::default()
        })
        .await
    }
}
