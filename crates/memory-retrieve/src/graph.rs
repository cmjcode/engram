//! Graf proyek bergaya Obsidian: simpul proyek (padanan "note"), tag, dan entitas bersama;
//! sisi dari tautan `[[...]]` (catatan proyek + teks episode), sebutan entitas proyek lain,
//! tag grup, dan entitas yang dipakai. Dihitung di Rust per permintaan karena datanya kecil
//! (metadata proyek + baris episode ringan tanpa teks).

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use memory_core::model::{Entity, EntityKind, ProjectMeta};
use memory_core::project::{project_key, same_project};
use memory_store::EpisodeLinkRow;
use serde::{Deserialize, Serialize};

use crate::{Result, Retriever};

/// Batas default simpul agar kanvas tetap terbaca.
pub const GRAPH_MAX_NODES: usize = 400;
/// Entitas terbanyak per proyek pada graf lokal.
const LOCAL_ENTITIES_PER_PROJECT: usize = 40;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GraphOpts {
    /// Proyek fokus untuk graf lokal; `None` = graf global.
    pub project: Option<String>,
    /// Kedalaman BFS graf lokal (1..=3).
    pub depth: usize,
    pub include_tags: bool,
    pub include_entities: bool,
    pub max_nodes: usize,
}

impl Default for GraphOpts {
    fn default() -> Self {
        Self { project: None, depth: 1, include_tags: true, include_entities: true, max_nodes: GRAPH_MAX_NODES }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GraphNode {
    /// `p:<kunci proyek>`, `t:<tag huruf kecil>`, atau `e:<id entitas>`.
    pub id: String,
    pub label: String,
    /// `project` | `tag` | `entity`
    pub kind: String,
    /// Jenis entitas (`tool`, `org`, ...) untuk simpul entitas.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_kind: Option<String>,
    /// ID entitas asli (untuk membuka detail entitas).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entity_id: Option<String>,
    /// Jumlah episode proyek (atau jumlah proyek untuk tag/entitas).
    pub weight: u32,
    /// Proyek punya catatan/metadata.
    #[serde(default)]
    pub has_meta: bool,
    /// Proyek hanya dikenal dari tautan `[[...]]` (belum ada memori), seperti tautan
    /// "belum dibuat" di Obsidian.
    #[serde(default)]
    pub missing: bool,
    /// Jumlah sisi yang menyentuh simpul.
    pub degree: u32,
    /// Waktu memori terakhir (ISO 8601) dari episode proyek; tag/entitas mewarisi nilai terbaru
    /// dari proyek tetangganya. `None` = belum ada memori (mis. simpul "missing").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
    /// `link` (tautan `[[...]]`) | `mention` (sebutan entitas proyek lain) | `tag` | `entity`
    pub kind: String,
    pub weight: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ProjectGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    /// ID simpul fokus (graf lokal).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub focus: Option<String>,
    /// Simpul dipotong karena melebihi batas.
    pub truncated: bool,
    /// Jumlah simpul sebelum dipotong.
    pub total_nodes: usize,
}

fn kind_str(k: EntityKind) -> String {
    serde_json::to_value(k).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default()
}

/// Jenis entitas yang tampil di graf global (berkas/komponen hanya di graf lokal).
fn global_entity_kind(k: EntityKind) -> bool {
    matches!(k, EntityKind::Project | EntityKind::Tool | EntityKind::Org | EntityKind::Person | EntityKind::Place)
}

/// Daftar proyek kanonik: (kunci, label). Pencarian memakai kunci persis lalu `same_project`.
#[derive(Default)]
struct Canon(Vec<(String, String)>);

impl Canon {
    fn find(&self, name: &str) -> Option<String> {
        let k = project_key(name);
        self.0.iter().find(|(ck, _)| *ck == k).or_else(|| self.0.iter().find(|(_, l)| same_project(l, name))).map(|(ck, _)| ck.clone())
    }
}

#[derive(Default)]
struct Builder {
    canon: Canon,
    nodes: BTreeMap<String, GraphNode>,
    edges: BTreeMap<(String, String, String), u32>,
}

fn pid(k: &str) -> String {
    format!("p:{k}")
}

impl Builder {
    fn node(&mut self, id: &str, label: &str, kind: &str) -> &mut GraphNode {
        self.nodes.entry(id.to_string()).or_insert_with(|| GraphNode {
            id: id.to_string(),
            label: label.to_string(),
            kind: kind.to_string(),
            entity_kind: None,
            entity_id: None,
            weight: 0,
            has_meta: false,
            missing: false,
            degree: 0,
            last_at: None,
        })
    }

    /// Simpul proyek untuk `name`; nama yang belum dikenal didaftarkan (opsional sebagai "missing").
    fn project(&mut self, name: &str, missing_if_new: bool) -> String {
        if let Some(k) = self.canon.find(name) {
            return pid(&k);
        }
        let k = project_key(name);
        self.canon.0.push((k.clone(), name.to_string()));
        let id = pid(&k);
        self.node(&id, name, "project").missing = missing_if_new;
        id
    }

    fn edge(&mut self, from: &str, to: &str, kind: &str, w: u32) {
        if from == to {
            return;
        }
        *self.edges.entry((from.to_string(), to.to_string(), kind.to_string())).or_insert(0) += w;
    }
}

/// Bangun graf dari metadata proyek, baris episode ringan, dan entitas yang disebut episode.
pub fn build_project_graph(metas: &[ProjectMeta], rows: &[EpisodeLinkRow], entities: &[Entity], opts: &GraphOpts) -> ProjectGraph {
    let mut b = Builder::default();
    // Nama kanonik: metadata lebih dulu (mis. `BACKEND/api-x`), lalu nama pertama di episode.
    for m in metas {
        let id = b.project(&m.project, false);
        b.node(&id, &m.project, "project").has_meta = true;
    }
    for r in rows {
        if let Some(p) = r.project.as_deref() {
            let id = b.project(p, false);
            let n = b.node(&id, p, "project");
            n.weight += 1;
            n.missing = false;
            // String ISO dari formatter yang sama bisa dibandingkan secara leksikografis.
            if !r.created_at.is_empty() && n.last_at.as_deref().is_none_or(|c| c < r.created_at.as_str()) {
                n.last_at = Some(r.created_at.clone());
            }
        }
    }

    // Catatan proyek: tautan + tag. Target `[[...]]` yang belum dikenal menjadi simpul "missing".
    for m in metas {
        let from = b.project(&m.project, false);
        for l in &m.links {
            let to = b.project(l, true);
            b.edge(&from, &to, "link", 1);
        }
        if opts.include_tags {
            for t in &m.tags {
                let tid = format!("t:{}", t.to_lowercase());
                b.node(&tid, &format!("#{t}"), "tag").weight += 1;
                b.edge(&from, &tid, "tag", 1);
            }
        }
    }

    // Episode: tautan eksplisit dan sebutan entitas.
    let ent: HashMap<&str, &Entity> = entities.iter().filter(|e| !e.is_self).map(|e| (e.id.as_str(), e)).collect();
    // simpul proyek -> entitas -> jumlah episode
    let mut uses: BTreeMap<String, BTreeMap<String, u32>> = BTreeMap::new();
    for r in rows {
        let Some(p) = r.project.as_deref() else { continue };
        let from = b.project(p, false);
        for l in &r.links {
            if same_project(l, p) {
                continue;
            }
            let to = b.project(l, true);
            b.edge(&from, &to, "link", 1);
        }
        let mut seen: HashSet<&str> = HashSet::new();
        for eid in &r.entity_ids {
            let Some(e) = ent.get(eid.as_str()) else { continue };
            if !seen.insert(eid.as_str()) {
                continue;
            }
            // Entitas proyek yang cocok dengan proyek lain yang dikenal = sebutan antar proyek.
            if e.kind == EntityKind::Project
                && let Some(k) = std::iter::once(&e.name).chain(e.aliases.iter()).find_map(|n| b.canon.find(n))
            {
                b.edge(&from, &pid(&k), "mention", 1);
                continue;
            }
            *uses.entry(from.clone()).or_default().entry(eid.clone()).or_insert(0) += 1;
        }
    }

    // Entitas: global hanya yang dipakai >= 2 proyek; lokal juga entitas teratas milik proyek fokus.
    let focus = opts.project.as_deref().map(|p| b.canon.find(p).map(|k| pid(&k)).unwrap_or_else(|| pid(&project_key(p))));
    if opts.include_entities {
        let mut projects_per_entity: HashMap<&str, u32> = HashMap::new();
        for m in uses.values() {
            for eid in m.keys() {
                *projects_per_entity.entry(eid.as_str()).or_insert(0) += 1;
            }
        }
        let focus_top: HashSet<String> = focus
            .as_ref()
            .and_then(|f| uses.get(f))
            .map(|m| {
                let mut v: Vec<(&String, &u32)> = m.iter().collect();
                v.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
                v.into_iter().take(LOCAL_ENTITIES_PER_PROJECT).map(|(k, _)| k.clone()).collect()
            })
            .unwrap_or_default();
        for (proj, m) in &uses {
            for (eid, n) in m {
                let e = ent[eid.as_str()];
                let shared = projects_per_entity.get(eid.as_str()).copied().unwrap_or(0) >= 2 && global_entity_kind(e.kind);
                let local = focus.as_deref() == Some(proj.as_str()) && focus_top.contains(eid);
                if !shared && !local {
                    continue;
                }
                let id = format!("e:{eid}");
                let node = b.node(&id, &e.name, "entity");
                node.entity_kind = Some(kind_str(e.kind));
                node.entity_id = Some(eid.clone());
                node.weight += 1;
                b.edge(proj, &id, "entity", *n);
            }
        }
    }

    let mut edges: Vec<GraphEdge> = b.edges.into_iter().map(|((from, to, kind), weight)| GraphEdge { from, to, kind, weight }).collect();
    let mut nodes = b.nodes;
    for e in &edges {
        for id in [&e.from, &e.to] {
            if let Some(n) = nodes.get_mut(id) {
                n.degree += 1;
            }
        }
    }
    // Tag/entitas: terakhir aktif = memori terbaru dari proyek yang memakainya, agar filter
    // aktivitas di UI menyembunyikan simpul sekunder yang basi secara konsisten.
    let mut inherited: HashMap<String, String> = HashMap::new();
    for e in &edges {
        if e.kind != "tag" && e.kind != "entity" {
            continue;
        }
        let Some(at) = nodes.get(&e.from).and_then(|n| n.last_at.clone()) else { continue };
        let cur = inherited.entry(e.to.clone()).or_default();
        if *cur < at {
            *cur = at;
        }
    }
    for (id, at) in inherited {
        if let Some(n) = nodes.get_mut(&id) {
            n.last_at = Some(at);
        }
    }

    // Graf lokal: BFS tak berarah dari proyek fokus.
    let mut keep: Option<HashSet<String>> = None;
    if let Some(f) = &focus {
        if !nodes.contains_key(f) {
            return ProjectGraph { focus: Some(f.clone()), ..Default::default() };
        }
        let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
        for e in &edges {
            adj.entry(e.from.as_str()).or_default().push(e.to.as_str());
            adj.entry(e.to.as_str()).or_default().push(e.from.as_str());
        }
        let mut seen: HashSet<String> = HashSet::from([f.clone()]);
        let mut q: VecDeque<(&str, usize)> = VecDeque::from([(f.as_str(), 0)]);
        while let Some((n, d)) = q.pop_front() {
            if d >= opts.depth.clamp(1, 3) {
                continue;
            }
            for &m in adj.get(n).map(Vec::as_slice).unwrap_or(&[]) {
                if seen.insert(m.to_string()) {
                    q.push_back((m, d + 1));
                }
            }
        }
        keep = Some(seen);
    }
    let mut list: Vec<GraphNode> = nodes.into_values().filter(|n| keep.as_ref().is_none_or(|k| k.contains(&n.id))).collect();
    let total_nodes = list.len();
    let max = opts.max_nodes.max(1);
    let truncated = list.len() > max;
    if truncated {
        // Pertahankan fokus, lalu proyek, lalu simpul berderajat tinggi.
        let is_focus = |n: &GraphNode| focus.as_deref() == Some(n.id.as_str());
        list.sort_by(|a, b| {
            is_focus(b)
                .cmp(&is_focus(a))
                .then((b.kind == "project").cmp(&(a.kind == "project")))
                .then(b.degree.cmp(&a.degree))
                .then(b.weight.cmp(&a.weight))
                .then(a.id.cmp(&b.id))
        });
        list.truncate(max);
        list.sort_by(|a, b| a.id.cmp(&b.id));
    }
    let ids: HashSet<&str> = list.iter().map(|n| n.id.as_str()).collect();
    edges.retain(|e| ids.contains(e.from.as_str()) && ids.contains(e.to.as_str()));
    ProjectGraph { nodes: list, edges, focus, truncated, total_nodes }
}

/// Proyek tetangga langsung `name` lewat tautan & sebutan (dua arah, tanpa tag/entitas),
/// terberat dulu. Hasil: (nama proyek, `link` | `mention`).
pub fn graph_neighbors(g: &ProjectGraph, name: &str) -> Vec<(String, String)> {
    let Some(me) = g.nodes.iter().find(|n| n.kind == "project" && same_project(&n.label, name)) else { return vec![] };
    let mut out: Vec<(String, String, u32)> = Vec::new();
    for e in g.edges.iter().filter(|e| e.kind == "link" || e.kind == "mention") {
        let other = if e.from == me.id {
            &e.to
        } else if e.to == me.id {
            &e.from
        } else {
            continue;
        };
        let Some(n) = g.nodes.iter().find(|n| &n.id == other) else { continue };
        match out.iter_mut().find(|(l, _, _)| same_project(l, &n.label)) {
            Some(x) => {
                x.2 += e.weight;
                if e.kind == "link" {
                    x.1 = "link".into();
                }
            }
            None => out.push((n.label.clone(), e.kind.clone(), e.weight)),
        }
    }
    out.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.cmp(&b.0)));
    out.into_iter().map(|(l, k, _)| (l, k)).collect()
}

impl Retriever {
    /// Graf proyek subjek (global, atau lokal bila `opts.project` diisi).
    pub async fn project_graph(&self, subject: &str, opts: &GraphOpts) -> Result<ProjectGraph> {
        let (metas, rows) =
            tokio::try_join!(async { Ok::<_, crate::RetrieveError>(self.store.list_project_meta(subject).await?) }, async {
                Ok::<_, crate::RetrieveError>(self.store.episode_link_rows(subject).await?)
            },)?;
        let mut ids: Vec<String> = rows.iter().flat_map(|r| r.entity_ids.iter().cloned()).collect::<HashSet<_>>().into_iter().collect();
        ids.sort();
        let entities = self.store.entities_by_ids(&ids).await?;
        Ok(build_project_graph(&metas, &rows, &entities, opts))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(p: &str, notes: &str, tags: &[&str]) -> ProjectMeta {
        ProjectMeta::build("s", p, None, notes, &tags.iter().map(|t| t.to_string()).collect::<Vec<_>>(), None)
    }
    fn row(p: &str, ents: &[&str], links: &[&str]) -> EpisodeLinkRow {
        EpisodeLinkRow {
            project: Some(p.into()),
            entity_ids: ents.iter().map(|s| s.to_string()).collect(),
            links: links.iter().map(|s| s.to_string()).collect(),
            created_at: String::new(),
        }
    }
    fn row_at(p: &str, ents: &[&str], links: &[&str], at: &str) -> EpisodeLinkRow {
        EpisodeLinkRow { created_at: at.into(), ..row(p, ents, links) }
    }
    fn ent(id: &str, name: &str, kind: EntityKind) -> Entity {
        Entity {
            id: id.into(),
            subject: "s".into(),
            kind,
            name: name.into(),
            aliases: vec![],
            description: String::new(),
            status: Default::default(),
            merged_into: None,
            is_self: false,
            gender: None,
            mention_count: 1,
            episode_ids: vec![],
            created_at: String::new(),
        }
    }
    fn edge<'a>(g: &'a ProjectGraph, from: &str, to: &str, kind: &str) -> Option<&'a GraphEdge> {
        g.edges.iter().find(|e| e.from == from && e.to == to && e.kind == kind)
    }

    fn sample() -> (Vec<ProjectMeta>, Vec<EpisodeLinkRow>, Vec<Entity>) {
        let metas =
            vec![meta("BACKEND/api-x", "Konsumen [[LLM-MEMORY]] dan [[Rencana Baru]]", &["OPERON"]), meta("TABULAR", "", &["operon"])];
        let rows = vec![
            row_at("LLM-MEMORY", &["e_rust", "e_tab", "e_file"], &["TABULAR"], "2026-11-01T08:00:00Z"),
            row_at("LLM-MEMORY", &["e_rust"], &["tabular", "llm-memory"], "2026-11-10T09:30:00Z"),
            row_at("TABULAR", &["e_rust", "e_file2"], &[], "2026-03-02T10:00:00Z"),
            row("api-x", &[], &[]),
        ];
        let entities = vec![
            ent("e_rust", "Rust", EntityKind::Tool),
            ent("e_tab", "TABULAR", EntityKind::Project),
            ent("e_file", "src/main.rs", EntityKind::Component),
            ent("e_file2", "src/lib.rs", EntityKind::Component),
        ];
        (metas, rows, entities)
    }

    #[test]
    fn global_graph_edges_and_nodes() {
        let (m, r, e) = sample();
        let g = build_project_graph(&m, &r, &e, &GraphOpts::default());
        // `api-x` menyatu dengan `BACKEND/api-x` (nama kanonik dari metadata).
        let apix = g.nodes.iter().find(|n| n.id == "p:backend/api-x").unwrap();
        assert_eq!((apix.label.as_str(), apix.weight, apix.has_meta), ("BACKEND/api-x", 1, true));
        assert!(!g.nodes.iter().any(|n| n.id == "p:api-x"));
        // Tautan dari catatan dan dari episode (dijumlahkan, tanpa tautan ke diri sendiri).
        assert_eq!(edge(&g, "p:backend/api-x", "p:llm-memory", "link").unwrap().weight, 1);
        assert_eq!(edge(&g, "p:llm-memory", "p:tabular", "link").unwrap().weight, 2);
        assert!(edge(&g, "p:llm-memory", "p:llm-memory", "link").is_none());
        // Tautan ke proyek yang belum ada = simpul "missing"; proyek dengan episode bukan missing.
        assert!(g.nodes.iter().find(|n| n.id == "p:rencanabaru").unwrap().missing);
        assert!(!g.nodes.iter().find(|n| n.id == "p:llm-memory").unwrap().missing);
        // Entitas proyek yang cocok dengan proyek lain = sebutan, bukan simpul entitas.
        assert_eq!(edge(&g, "p:llm-memory", "p:tabular", "mention").unwrap().weight, 1);
        assert!(!g.nodes.iter().any(|n| n.id == "e:e_tab"));
        // Tag digabung tanpa membedakan huruf besar-kecil.
        assert_eq!(g.nodes.iter().find(|n| n.id == "t:operon").unwrap().weight, 2);
        // Entitas bersama (Rust dipakai 2 proyek) tampil; berkas tidak tampil di graf global.
        assert_eq!(edge(&g, "p:llm-memory", "e:e_rust", "entity").unwrap().weight, 2);
        assert!(edge(&g, "p:tabular", "e:e_rust", "entity").is_some());
        assert!(!g.nodes.iter().any(|n| n.id == "e:e_file"));
        assert!(g.focus.is_none() && !g.truncated);
        assert_eq!(g.total_nodes, g.nodes.len());
    }

    #[test]
    fn last_at_is_latest_memory_and_inherited_by_tags() {
        let (m, r, e) = sample();
        let g = build_project_graph(&m, &r, &e, &GraphOpts::default());
        let at = |id: &str| g.nodes.iter().find(|n| n.id == id).unwrap().last_at.clone();
        // Proyek: episode terbaru menang, bukan yang pertama.
        assert_eq!(at("p:llm-memory").as_deref(), Some("2026-11-10T09:30:00Z"));
        assert_eq!(at("p:tabular").as_deref(), Some("2026-03-02T10:00:00Z"));
        // Episode tanpa waktu dan target [[...]] yang belum punya memori: tidak diketahui.
        assert_eq!(at("p:backend/api-x"), None);
        assert_eq!(at("p:rencanabaru"), None);
        // Tag & entitas mewarisi waktu terbaru dari proyek yang memakainya.
        assert_eq!(at("t:operon").as_deref(), Some("2026-03-02T10:00:00Z"));
        assert_eq!(at("e:e_rust").as_deref(), Some("2026-11-10T09:30:00Z"));
    }

    #[test]
    fn local_graph_depth_and_truncation() {
        let (m, r, e) = sample();
        let opts = GraphOpts { project: Some("tabular".into()), depth: 1, include_tags: false, ..Default::default() };
        let g = build_project_graph(&m, &r, &e, &opts);
        assert_eq!(g.focus.as_deref(), Some("p:tabular"));
        let ids: Vec<&str> = g.nodes.iter().map(|n| n.id.as_str()).collect();
        // Tetangga langsung: LLM-MEMORY (tautan), Rust, berkas milik TABULAR; bukan api-x (kedalaman 2).
        assert!(ids.contains(&"p:llm-memory") && ids.contains(&"e:e_rust") && ids.contains(&"e:e_file2"), "{ids:?}");
        assert!(!ids.contains(&"p:backend/api-x") && !ids.contains(&"t:operon"));
        let g2 = build_project_graph(&m, &r, &e, &GraphOpts { depth: 2, ..opts.clone() });
        assert!(g2.nodes.iter().any(|n| n.id == "p:backend/api-x"));
        // Batas simpul: fokus tetap ada, sisi ke simpul terbuang ikut dibuang.
        let g3 = build_project_graph(&m, &r, &e, &GraphOpts { depth: 2, max_nodes: 2, ..opts });
        assert!(g3.truncated && g3.nodes.len() == 2 && g3.nodes.iter().any(|n| n.id == "p:tabular"));
        assert!(g3.edges.iter().all(|e| g3.nodes.iter().any(|n| n.id == e.from) && g3.nodes.iter().any(|n| n.id == e.to)));
        // Proyek tak dikenal: graf kosong.
        assert!(build_project_graph(&m, &r, &e, &GraphOpts { project: Some("zzz".into()), ..Default::default() }).nodes.is_empty());
        // Tetangga untuk project_context.
        let g = build_project_graph(&m, &r, &e, &GraphOpts::default());
        assert_eq!(graph_neighbors(&g, "TABULAR").first(), Some(&("LLM-MEMORY".to_string(), "link".to_string())));
    }
}
