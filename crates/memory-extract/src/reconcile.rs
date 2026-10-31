//! Rekonsiliasi fakta bitemporal (fungsi murni, tanpa I/O).
//!
//! Fakta baru dibandingkan dengan fakta yang sudah ada untuk entitas &
//! kategori yang sama. Fakta lama TIDAK pernah dihapus atau ditimpa:
//! - objek sama & periode bersinggungan  -> bukti digabung (fakta diperkuat);
//! - objek beda & eksklusif & berurutan  -> fakta lama ditutup `valid_to`,
//!   dihubungkan `superseded_by` / `supersedes`;
//! - objek beda & eksklusif & tidak bisa diurutkan -> keduanya `conflict`,
//!   masuk antrean review manusia.

use chrono::NaiveDate;
use serde_json::json;

use memory_core::model::{Evidence, Fact, FactStatus, ReviewItem, ReviewKind, ReviewStatus};
use memory_core::text::normalize;

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Insert(Box<Fact>),
    /// Perbarui fakta yang sudah ada (seluruh record baru).
    Update(Box<Fact>),
    Review(Box<ReviewItem>),
}

/// Toleransi tumpang tindih akibat batas waktu kabur (mis. "umur 20" vs "2000").
const FUZZY_OVERLAP_DAYS: i64 = 400;

fn date(s: &Option<String>) -> Option<NaiveDate> {
    s.as_deref().and_then(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok())
}

fn iso(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

pub fn same_object(a: &Fact, b: &Fact) -> bool {
    match (&a.object_entity, &b.object_entity) {
        (Some(x), Some(y)) => x == y,
        _ => normalize(&a.object_text) == normalize(&b.object_text),
    }
}

/// Rentang [from, to] dengan batas terbuka bila tidak diketahui.
fn overlaps(a: &Fact, b: &Fact) -> bool {
    let (af, at, bf, bt) = (date(&a.valid_from), date(&a.valid_to), date(&b.valid_from), date(&b.valid_to));
    let a_before_b_ends = match (af, bt) {
        (Some(x), Some(y)) => x <= y,
        _ => true,
    };
    let b_before_a_ends = match (bf, at) {
        (Some(x), Some(y)) => x <= y,
        _ => true,
    };
    a_before_b_ends && b_before_a_ends
}

fn merge_evidence(into: &mut Fact, from: &Fact) {
    for ev in &from.evidence {
        if !into.evidence.iter().any(|e| e.episode_id == ev.episode_id && e.quote == ev.quote) {
            into.evidence.push(Evidence { episode_id: ev.episode_id.clone(), quote: ev.quote.clone() });
        }
    }
    into.confidence = (into.confidence.max(from.confidence) + 0.05).min(0.99);
    // Perluas rentang bila fakta baru memberi batas yang lebih awal/lambat.
    if let (Some(nf), cur) = (date(&from.valid_from), date(&into.valid_from))
        && cur.is_none_or(|c| nf < c) {
            into.valid_from = Some(iso(nf));
        }
    if into.valid_to.is_some() {
        match (date(&from.valid_to), from.ongoing) {
            (_, true) => {
                into.valid_to = None;
                into.ongoing = true;
            }
            (Some(nt), _) if date(&into.valid_to).is_some_and(|c| nt > c) => into.valid_to = Some(iso(nt)),
            _ => {}
        }
    }
}

fn conflict_review(a: &Fact, b: &Fact, now: &str) -> ReviewItem {
    ReviewItem {
        id: crate::det_id(&["conflict", &a.id, &b.id]),
        subject: a.subject.clone(),
        kind: ReviewKind::Conflict,
        summary: format!(
            "Dua fakta '{}' bertentangan: \"{}\" ({}) vs \"{}\" ({}). Waktunya tidak bisa diurutkan.",
            a.category,
            a.object_text,
            a.time.label(),
            b.object_text,
            b.time.label()
        ),
        targets: vec![a.id.clone(), b.id.clone()],
        payload: json!({"existing": a.id, "new": b.id}),
        status: ReviewStatus::Open,
        note: None,
        created_at: now.to_string(),
        resolved_at: None,
    }
}

/// Tutup `older` sebelum `newer` dimulai dan hubungkan keduanya.
fn supersede(older: &mut Fact, newer: &mut Fact) {
    if let Some(nf) = date(&newer.valid_from) {
        let close = nf.pred_opt().unwrap_or(nf);
        let should_close = older.valid_to.is_none() || date(&older.valid_to).is_some_and(|t| t >= nf);
        if should_close {
            older.valid_to = Some(iso(close));
            older.ongoing = false;
        }
    }
    if older.superseded_by.is_none() {
        older.superseded_by = Some(newer.id.clone());
    }
    if newer.supersedes.is_none() {
        newer.supersedes = Some(older.id.clone());
    }
}

enum Order {
    /// existing sebelum new
    ExistingFirst,
    NewFirst,
    Conflict,
}

fn order(e: &Fact, n: &Fact) -> Order {
    let (ef, et, nf, nt) = (date(&e.valid_from), date(&e.valid_to), date(&n.valid_from), date(&n.valid_to));
    match (ef, nf) {
        (Some(ef), Some(nf)) => {
            if (ef - nf).num_days().abs() <= 30 {
                return Order::Conflict;
            }
            let (older_to, newer_from, existing_first) = if ef < nf { (et, nf, true) } else { (nt, ef, false) };
            // Tumpang tindih besar antara rentang tertutup = klaim bertentangan.
            if let Some(ot) = older_to
                && (ot - newer_from).num_days() > FUZZY_OVERLAP_DAYS {
                    return Order::Conflict;
                }
            if existing_first { Order::ExistingFirst } else { Order::NewFirst }
        }
        // Satu sisi hanya punya batas akhir ("dulu"): urut bila akhirnya sebelum awal sisi lain.
        (None, Some(nf)) => match et {
            Some(et) if et < nf => Order::ExistingFirst,
            _ => Order::Conflict,
        },
        (Some(ef), None) => match nt {
            Some(nt) if nt < ef => Order::NewFirst,
            _ => Order::Conflict,
        },
        (None, None) => Order::Conflict,
    }
}

/// Rekonsiliasi `new` terhadap `existing` (fakta aktif/konflik dengan entitas
/// & kategori sama). `existing` diperbarui di tempat agar fakta berikutnya di
/// transaksi yang sama melihat keadaan terbaru.
pub fn reconcile(mut new: Fact, existing: &mut Vec<Fact>, now: &str) -> Vec<Action> {
    let mut actions = Vec::new();
    let related: Vec<usize> = existing
        .iter()
        .enumerate()
        .filter(|(_, e)| e.entity_id == new.entity_id && e.category == new.category && matches!(e.status, FactStatus::Active | FactStatus::Conflict))
        .map(|(i, _)| i)
        .collect();

    // 1. Objek sama & periode bersinggungan: perkuat fakta lama.
    for &i in &related {
        if same_object(&existing[i], &new) && overlaps(&existing[i], &new) && new.status == FactStatus::Active {
            merge_evidence(&mut existing[i], &new);
            actions.push(Action::Update(Box::new(existing[i].clone())));
            return actions;
        }
    }

    // 2. Objek berbeda untuk kategori eksklusif.
    if new.exclusive && new.status == FactStatus::Active {
        for &i in &related {
            let e = &existing[i];
            if same_object(e, &new) || !e.exclusive || e.status != FactStatus::Active {
                continue;
            }
            match order(e, &new) {
                Order::ExistingFirst => {
                    let mut e = existing[i].clone();
                    supersede(&mut e, &mut new);
                    existing[i] = e.clone();
                    actions.push(Action::Update(Box::new(e)));
                }
                Order::NewFirst => {
                    let mut e = existing[i].clone();
                    supersede(&mut new, &mut e);
                    existing[i] = e.clone();
                    actions.push(Action::Update(Box::new(e)));
                }
                Order::Conflict => {
                    let mut e = existing[i].clone();
                    e.status = FactStatus::Conflict;
                    new.status = FactStatus::Conflict;
                    existing[i] = e.clone();
                    actions.push(Action::Update(Box::new(e.clone())));
                    actions.push(Action::Review(Box::new(conflict_review(&e, &new, now))));
                }
            }
        }
    }

    existing.push(new.clone());
    actions.insert(0, Action::Insert(Box::new(new)));
    actions
}

#[cfg(test)]
mod tests {
    use super::*;
    use memory_core::model::FactKind;
    use memory_core::time::TimeRef;

    fn f(id: &str, obj: &str, from: Option<&str>, to: Option<&str>) -> Fact {
        Fact {
            id: id.into(),
            subject: "s".into(),
            entity_id: "self".into(),
            predicate: "mendukung".into(),
            category: "afiliasi_politik".into(),
            object_entity: None,
            object_text: obj.into(),
            kind: FactKind::Preference,
            exclusive: true,
            time: TimeRef::Unknown,
            valid_from: from.map(Into::into),
            valid_to: to.map(Into::into),
            ongoing: to.is_none() && from.is_some(),
            time_confidence: 0.8,
            observed_at: "2024-05-01".into(),
            confidence: 0.8,
            evidence: vec![Evidence { episode_id: format!("ep-{id}"), quote: "q".into() }],
            status: FactStatus::Active,
            superseded_by: None,
            supersedes: None,
            created_at: String::new(),
        }
    }

    fn find<'a>(v: &'a [Fact], id: &str) -> &'a Fact {
        v.iter().find(|x| x.id == id).unwrap()
    }

    #[test]
    fn party_a_at_20_then_party_b_at_40() {
        let mut ex = vec![];
        reconcile(f("a", "Partai A", Some("1999-03-10"), None), &mut ex, "now");
        let acts = reconcile(f("b", "Partai B", Some("2019-03-10"), None), &mut ex, "now");
        assert!(acts.iter().all(|a| !matches!(a, Action::Review(_))));
        let a = find(&ex, "a");
        let b = find(&ex, "b");
        assert_eq!(a.valid_to.as_deref(), Some("2019-03-09"));
        assert_eq!(a.superseded_by.as_deref(), Some("b"));
        assert_eq!(b.supersedes.as_deref(), Some("a"));
        assert!(b.is_current() && !a.is_current());
        assert!(a.valid_at("2005-01-01") && !b.valid_at("2005-01-01"));
    }

    #[test]
    fn same_story_in_reverse_order() {
        let mut ex = vec![];
        reconcile(f("b", "Partai B", Some("2019-03-10"), None), &mut ex, "now");
        reconcile(f("a", "Partai A", Some("1999-03-10"), None), &mut ex, "now");
        let a = find(&ex, "a");
        assert_eq!(a.valid_to.as_deref(), Some("2019-03-09"));
        assert_eq!(a.superseded_by.as_deref(), Some("b"));
        assert!(find(&ex, "b").is_current());
    }

    #[test]
    fn dulu_vs_sekarang_is_sequential() {
        let mut ex = vec![];
        // "dulu Partai A" -> Past: end = observed-1
        reconcile(f("a", "Partai A", None, Some("2024-04-30")), &mut ex, "now");
        let acts = reconcile(f("b", "Partai B", Some("2024-05-01"), None), &mut ex, "now");
        assert!(acts.iter().all(|a| !matches!(a, Action::Review(_))), "{acts:?}");
        assert_eq!(find(&ex, "a").superseded_by.as_deref(), Some("b"));
    }

    #[test]
    fn undated_contradiction_goes_to_review() {
        let mut ex = vec![];
        reconcile(f("a", "Partai A", None, None), &mut ex, "now");
        let acts = reconcile(f("b", "Partai B", None, None), &mut ex, "now");
        assert!(acts.iter().any(|a| matches!(a, Action::Review(_))));
        assert_eq!(find(&ex, "a").status, FactStatus::Conflict);
        assert_eq!(find(&ex, "b").status, FactStatus::Conflict);
    }

    #[test]
    fn overlapping_closed_ranges_conflict_but_fuzzy_edges_do_not() {
        let mut ex = vec![];
        reconcile(f("a", "Partai A", Some("1999-01-01"), Some("2010-12-31")), &mut ex, "now");
        let acts = reconcile(f("b", "Partai B", Some("2005-01-01"), Some("2019-12-31")), &mut ex, "now");
        assert!(acts.iter().any(|a| matches!(a, Action::Review(_))));

        let mut ex = vec![];
        reconcile(f("a", "Partai A", Some("1999-01-01"), Some("2005-06-30")), &mut ex, "now");
        let acts = reconcile(f("b", "Partai B", Some("2005-01-01"), None), &mut ex, "now");
        assert!(acts.iter().all(|a| !matches!(a, Action::Review(_))));
        assert_eq!(find(&ex, "a").valid_to.as_deref(), Some("2004-12-31"));
    }

    #[test]
    fn retold_fact_strengthens_instead_of_duplicating() {
        let mut ex = vec![];
        reconcile(f("a", "Partai A", Some("1999-03-10"), None), &mut ex, "now");
        let acts = reconcile(f("a2", "partai a", None, None), &mut ex, "now");
        assert_eq!(acts.len(), 1);
        assert!(matches!(&acts[0], Action::Update(x) if x.id == "a" && x.evidence.len() == 2));
        assert_eq!(ex.len(), 1);
    }

    #[test]
    fn non_exclusive_facts_coexist() {
        let mut ex = vec![];
        let mut a = f("a", "memancing", Some("1990-01-01"), None);
        a.exclusive = false;
        a.category = "hobi".into();
        let mut b = f("b", "berkebun", Some("1995-01-01"), None);
        b.exclusive = false;
        b.category = "hobi".into();
        reconcile(a, &mut ex, "now");
        reconcile(b, &mut ex, "now");
        assert!(ex.iter().all(|x| x.is_current()));
    }
}
