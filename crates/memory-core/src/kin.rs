//! Ontologi kekerabatan Indonesia.
//!
//! Istilah yang diucapkan (pakde, bulik, mbah kakung, uwa, teteh, ipar, besan,
//! ...) dinormalisasi ke relasi kanonik beserta gender dan urutan lahir bila
//! istilahnya membawa informasi itu. Edge kekerabatan disimpan berarah:
//! `A -[relation]-> B` berarti "A adalah <relation> dari B".

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::text::normalize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Gender {
    Male,
    Female,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum KinRelation {
    Parent,
    Child,
    Sibling,
    Spouse,
    Grandparent,
    Grandchild,
    GreatGrandparent,
    GreatGrandchild,
    /// Saudara orang tua: pakde/bude (lebih tua), paklik/bulik (lebih muda).
    ParentSibling,
    /// Keponakan.
    SiblingChild,
    Cousin,
    ParentInLaw,
    ChildInLaw,
    SiblingInLaw,
    /// Besan: orang tua dari menantu.
    CoParentInLaw,
    AdoptiveParent,
    AdoptedChild,
}

impl KinRelation {
    /// Relasi kebalikan: bila A Parent B maka B Child A.
    pub fn inverse(self) -> KinRelation {
        use KinRelation::*;
        match self {
            Parent => Child,
            Child => Parent,
            Sibling => Sibling,
            Spouse => Spouse,
            Grandparent => Grandchild,
            Grandchild => Grandparent,
            GreatGrandparent => GreatGrandchild,
            GreatGrandchild => GreatGrandparent,
            ParentSibling => SiblingChild,
            SiblingChild => ParentSibling,
            Cousin => Cousin,
            ParentInLaw => ChildInLaw,
            ChildInLaw => ParentInLaw,
            SiblingInLaw => SiblingInLaw,
            CoParentInLaw => CoParentInLaw,
            AdoptiveParent => AdoptedChild,
            AdoptedChild => AdoptiveParent,
        }
    }

    /// Nama Indonesia untuk tampilan, memakai gender & urutan bila diketahui.
    pub fn label_id(self, gender: Option<Gender>, older: Option<bool>) -> &'static str {
        use Gender::*;
        use KinRelation::*;
        match (self, gender, older) {
            (Parent, Some(Male), _) => "ayah",
            (Parent, Some(Female), _) => "ibu",
            (Parent, None, _) => "orang tua",
            (Child, _, _) => "anak",
            (Sibling, Some(Male), Some(true)) => "kakak laki-laki",
            (Sibling, Some(Female), Some(true)) => "kakak perempuan",
            (Sibling, _, Some(true)) => "kakak",
            (Sibling, _, Some(false)) => "adik",
            (Sibling, _, None) => "saudara kandung",
            (Spouse, Some(Male), _) => "suami",
            (Spouse, Some(Female), _) => "istri",
            (Spouse, None, _) => "pasangan",
            (Grandparent, Some(Male), _) => "kakek",
            (Grandparent, Some(Female), _) => "nenek",
            (Grandparent, None, _) => "kakek/nenek",
            (Grandchild, _, _) => "cucu",
            (GreatGrandparent, _, _) => "buyut",
            (GreatGrandchild, _, _) => "cicit",
            (ParentSibling, Some(Male), Some(true)) => "pakde",
            (ParentSibling, Some(Female), Some(true)) => "bude",
            (ParentSibling, Some(Male), _) => "paman",
            (ParentSibling, Some(Female), _) => "bibi",
            (ParentSibling, None, _) => "paman/bibi",
            (SiblingChild, _, _) => "keponakan",
            (Cousin, _, _) => "sepupu",
            (ParentInLaw, _, _) => "mertua",
            (ChildInLaw, _, _) => "menantu",
            (SiblingInLaw, _, _) => "ipar",
            (CoParentInLaw, _, _) => "besan",
            (AdoptiveParent, _, _) => "orang tua angkat",
            (AdoptedChild, _, _) => "anak angkat",
        }
    }
}

/// Hasil normalisasi satu istilah kekerabatan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct KinTerm {
    pub relation: KinRelation,
    pub gender: Option<Gender>,
    /// Untuk Sibling/ParentSibling: lebih tua (true) atau lebih muda (false).
    pub older: Option<bool>,
}

const fn t(relation: KinRelation, gender: Option<Gender>, older: Option<bool>) -> KinTerm {
    KinTerm {
        relation,
        gender,
        older,
    }
}

/// Normalisasi istilah kekerabatan (Indonesia, Jawa, Sunda, Betawi, Inggris).
/// Istilah adalah peran orang itu terhadap ego, mis. "pakde" -> ParentSibling
/// laki-laki lebih tua.
pub fn normalize_kin_term(term: &str) -> Option<KinTerm> {
    use Gender::*;
    use KinRelation::*;
    let n = normalize(term);
    let n = n.trim_start_matches("sang ").trim();
    // Frasa multi-kata dulu.
    let phrase = match n {
        "mbah kakung" | "mbah lanang" | "eyang kakung" | "simbah kakung" | "kakek" | "aki"
        | "engkong" | "grandfather" | "opa" => Some(t(Grandparent, Some(Male), None)),
        "mbah putri" | "mbah wedok" | "eyang putri" | "simbah putri" | "nenek" | "nini"
        | "enyak nenek" | "grandmother" | "oma" | "uti" => Some(t(Grandparent, Some(Female), None)),
        "anak angkat" | "anak pupon" | "anak asuh" => Some(t(AdoptedChild, None, None)),
        "bapak angkat" | "ayah angkat" => Some(t(AdoptiveParent, Some(Male), None)),
        "ibu angkat" => Some(t(AdoptiveParent, Some(Female), None)),
        "pak de" | "pak dhe" | "pakdhe" | "uwak lanang" => {
            Some(t(ParentSibling, Some(Male), Some(true)))
        }
        "bu de" | "bu dhe" | "budhe" | "mbokde" | "mbok de" | "uwak wadon" => {
            Some(t(ParentSibling, Some(Female), Some(true)))
        }
        "pak lik" | "pak le" | "paklek" | "pakle" => {
            Some(t(ParentSibling, Some(Male), Some(false)))
        }
        "bu lik" | "bu le" | "bulek" | "mbok lik" | "buke" => {
            Some(t(ParentSibling, Some(Female), Some(false)))
        }
        "kakak ipar" | "adik ipar" | "ipar" | "brother in law" | "sister in law" => {
            Some(t(SiblingInLaw, None, None))
        }
        "anak menantu" | "menantu" | "mantu" => Some(t(ChildInLaw, None, None)),
        "bapak mertua" | "mertua laki laki" => Some(t(ParentInLaw, Some(Male), None)),
        "ibu mertua" | "mertua perempuan" => Some(t(ParentInLaw, Some(Female), None)),
        "kakak laki laki" | "abang" | "bang" | "kang" | "akang" | "aa" | "kakang" => {
            Some(t(Sibling, Some(Male), Some(true)))
        }
        "kakak perempuan" | "teteh" | "teh" | "ceu" | "ceuceu" | "yayu" | "mpok" => {
            Some(t(Sibling, Some(Female), Some(true)))
        }
        "adik laki laki" => Some(t(Sibling, Some(Male), Some(false))),
        "adik perempuan" => Some(t(Sibling, Some(Female), Some(false))),
        "saudara sepupu" | "sepupu" | "misanan" | "mindoan" | "cousin" => {
            Some(t(Cousin, None, None))
        }
        _ => None,
    };
    if phrase.is_some() {
        return phrase;
    }
    let head = n.split_whitespace().next()?;
    let r = match head {
        "ayah" | "bapak" | "bapa" | "bapake" | "papa" | "papi" | "abah" | "abi" | "babe"
        | "rama" | "apa" | "father" | "dad" => t(Parent, Some(Male), None),
        "ibu" | "emak" | "mak" | "mamak" | "mama" | "mami" | "umi" | "ummi" | "simbok" | "mbok"
        | "biyung" | "ambu" | "enyak" | "nyak" | "mother" | "mom" => t(Parent, Some(Female), None),
        "orangtua" | "ortu" | "parent" => t(Parent, None, None),
        "anak" | "putra" | "putri" | "anakku" | "child" | "son" | "daughter" => {
            t(Child, None, None)
        }
        "kakak" | "mas" | "kak" | "mbak" | "mba" => {
            t(Sibling, gender_of_honorific(head), Some(true))
        }
        "adik" | "adek" | "dik" | "dek" | "adhi" => t(Sibling, None, Some(false)),
        "saudara" | "sedulur" | "dulur" | "sibling" => t(Sibling, None, None),
        "suami" | "bojo" | "laki" | "husband" => t(Spouse, Some(Male), None),
        "istri" | "isteri" | "garwo" | "garwa" | "bini" | "wife" => t(Spouse, Some(Female), None),
        "pasangan" | "spouse" => t(Spouse, None, None),
        "mbah" | "simbah" | "eyang" | "embah" | "engkong" | "kakek" | "nenek" => {
            t(Grandparent, gender_of_honorific(head), None)
        }
        "cucu" | "putu" | "grandchild" => t(Grandchild, None, None),
        "buyut" | "uyut" => t(GreatGrandparent, None, None),
        "cicit" | "buyutan" => t(GreatGrandchild, None, None),
        "pakde" | "pakdhe" | "siwo" | "siwa" | "uwak" | "uwa" | "wak" | "ua" => {
            t(ParentSibling, gender_of_honorific(head), Some(true))
        }
        "bude" | "budhe" => t(ParentSibling, Some(Female), Some(true)),
        "paklik" | "lik" | "om" | "paman" | "mamang" | "emang" | "uncle" => t(
            ParentSibling,
            Some(Male),
            match head {
                "paklik" | "lik" => Some(false),
                _ => None,
            },
        ),
        "bulik" | "tante" | "bibi" | "bi" | "aunt" => t(
            ParentSibling,
            Some(Female),
            match head {
                "bulik" => Some(false),
                _ => None,
            },
        ),
        "keponakan" | "ponakan" | "ponaan" | "nephew" | "niece" => t(SiblingChild, None, None),
        "mertua" | "maratua" | "morotuwo" => t(ParentInLaw, None, None),
        "besan" | "besanan" => t(CoParentInLaw, None, None),
        _ => return None,
    };
    Some(r)
}

fn gender_of_honorific(head: &str) -> Option<Gender> {
    match head {
        "mas" | "kakek" | "engkong" => Some(Gender::Male),
        "mbak" | "mba" | "nenek" | "bude" | "budhe" => Some(Gender::Female),
        "pakde" | "pakdhe" => Some(Gender::Male),
        _ => None,
    }
}

/// Edge kekerabatan generik untuk inferensi (ID node bebas).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KinEdge {
    pub from: String,
    pub to: String,
    pub relation: KinRelation,
}

/// Turunkan relasi yang tidak dinyatakan langsung dari edge yang ada.
/// Hanya mengembalikan edge yang belum ada. Aturan:
/// - Parent∘Parent = Grandparent
/// - Parent∘Grandparent = GreatGrandparent
/// - Sibling dari Parent = ParentSibling
/// - Child dari ParentSibling = Cousin
/// - Child dari Sibling = SiblingChild
/// - Parent dari Spouse = ParentInLaw
/// - Sibling dari Spouse / Spouse dari Sibling = SiblingInLaw
/// - Child dari Parent (bukan diri sendiri) = Sibling
pub fn infer_kin(edges: &[KinEdge]) -> Vec<KinEdge> {
    use KinRelation::*;
    use std::collections::HashSet;
    // Normalisasi: semua edge plus kebalikannya.
    let mut all: HashSet<KinEdge> = HashSet::new();
    for e in edges {
        all.insert(e.clone());
        all.insert(KinEdge {
            from: e.to.clone(),
            to: e.from.clone(),
            relation: e.relation.inverse(),
        });
    }
    let existing = all.clone();
    let mut out: HashSet<KinEdge> = HashSet::new();
    // "X rel Y" lookup: what is X to Y.
    let rel = |a: &str, r: KinRelation| -> Vec<String> {
        // semua X sehingga X r a
        existing
            .iter()
            .filter(|e| e.to == a && e.relation == r)
            .map(|e| e.from.clone())
            .collect()
    };
    let nodes: HashSet<String> = existing
        .iter()
        .flat_map(|e| [e.from.clone(), e.to.clone()])
        .collect();
    for ego in &nodes {
        let parents = rel(ego, Parent);
        for p in &parents {
            for gp in rel(p, Parent) {
                out.insert(KinEdge {
                    from: gp.clone(),
                    to: ego.clone(),
                    relation: Grandparent,
                });
                for ggp in rel(&gp, Parent) {
                    out.insert(KinEdge {
                        from: ggp,
                        to: ego.clone(),
                        relation: GreatGrandparent,
                    });
                }
            }
            for ps in rel(p, Sibling) {
                out.insert(KinEdge {
                    from: ps.clone(),
                    to: ego.clone(),
                    relation: ParentSibling,
                });
                for cousin in rel(&ps, Child) {
                    if &cousin != ego {
                        out.insert(KinEdge {
                            from: cousin,
                            to: ego.clone(),
                            relation: Cousin,
                        });
                    }
                }
            }
            for sib in rel(p, Child) {
                if &sib != ego {
                    out.insert(KinEdge {
                        from: sib,
                        to: ego.clone(),
                        relation: Sibling,
                    });
                }
            }
        }
        for sib in rel(ego, Sibling) {
            for nephew in rel(&sib, Child) {
                out.insert(KinEdge {
                    from: nephew,
                    to: ego.clone(),
                    relation: SiblingChild,
                });
            }
            for sp in rel(&sib, Spouse) {
                out.insert(KinEdge {
                    from: sp,
                    to: ego.clone(),
                    relation: SiblingInLaw,
                });
            }
        }
        for sp in rel(ego, Spouse) {
            for pil in rel(&sp, Parent) {
                out.insert(KinEdge {
                    from: pil,
                    to: ego.clone(),
                    relation: ParentInLaw,
                });
            }
            for sil in rel(&sp, Sibling) {
                out.insert(KinEdge {
                    from: sil,
                    to: ego.clone(),
                    relation: SiblingInLaw,
                });
            }
        }
    }
    let mut v: Vec<KinEdge> = out
        .into_iter()
        .filter(|e| e.from != e.to && !existing.contains(e))
        .collect();
    v.sort_by(|a, b| (&a.from, &a.to).cmp(&(&b.from, &b.to)));
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use KinRelation::*;

    #[test]
    fn javanese_and_sundanese_terms() {
        assert_eq!(
            normalize_kin_term("Pakde").unwrap(),
            KinTerm {
                relation: ParentSibling,
                gender: Some(Gender::Male),
                older: Some(true)
            }
        );
        assert_eq!(normalize_kin_term("bulik").unwrap().older, Some(false));
        assert_eq!(
            normalize_kin_term("Mbah Kakung").unwrap().relation,
            Grandparent
        );
        assert_eq!(
            normalize_kin_term("mbah putri").unwrap().gender,
            Some(Gender::Female)
        );
        assert_eq!(
            normalize_kin_term("teteh").unwrap(),
            KinTerm {
                relation: Sibling,
                gender: Some(Gender::Female),
                older: Some(true)
            }
        );
        assert_eq!(normalize_kin_term("uwa").unwrap().relation, ParentSibling);
        assert_eq!(normalize_kin_term("besan").unwrap().relation, CoParentInLaw);
        assert_eq!(normalize_kin_term("garwo").unwrap().relation, Spouse);
        assert_eq!(
            normalize_kin_term("anak pupon").unwrap().relation,
            AdoptedChild
        );
        assert!(normalize_kin_term("tetangga").is_none());
    }

    #[test]
    fn inverse_is_involution() {
        for r in [
            Parent,
            Grandparent,
            ParentSibling,
            ParentInLaw,
            AdoptiveParent,
            Cousin,
        ] {
            assert_eq!(r.inverse().inverse(), r);
        }
    }

    #[test]
    fn infers_grandparent_uncle_and_cousin() {
        let e = |a: &str, b: &str, r| KinEdge {
            from: a.into(),
            to: b.into(),
            relation: r,
        };
        let edges = vec![
            e("ayah", "ego", Parent),
            e("mbah", "ayah", Parent),
            e("pakde", "ayah", Sibling),
            e("sepupu", "pakde", Child),
            e("istri", "ego", Spouse),
            e("mertua", "istri", Parent),
        ];
        let inferred = infer_kin(&edges);
        let has = |a: &str, b: &str, r| inferred.contains(&e(a, b, r));
        assert!(has("mbah", "ego", Grandparent), "{inferred:?}");
        assert!(has("pakde", "ego", ParentSibling));
        assert!(has("sepupu", "ego", Cousin));
        assert!(has("mertua", "ego", ParentInLaw));
        assert!(!has("ego", "ego", Cousin));
    }

    #[test]
    fn labels_follow_javanese_order() {
        assert_eq!(
            ParentSibling.label_id(Some(Gender::Female), Some(true)),
            "bude"
        );
        assert_eq!(Sibling.label_id(None, Some(false)), "adik");
    }
}
