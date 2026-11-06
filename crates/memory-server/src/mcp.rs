//! Server MCP (Model Context Protocol): agent AI mengambil memori lewat tool.
//!
//! Transport: stdio (`engram mcp`, satu pesan JSON per baris) dan Streamable HTTP
//! (`POST /mcp` → respons JSON; `GET /mcp` → stream SSE keep-alive untuk klien
//! yang membukanya). Server ini stateless: tidak memakai `Mcp-Session-Id`, sesuai
//! spesifikasi 2026-07-28, dan tetap menerima klien 2024-11-05 s/d 2025-11-25.
//!
//! Logika temporal & guardrail ada di sisi server, sehingga LLM klien mana pun
//! mendapat konteks yang sama jujurnya. Deskripsi tool ditulis dalam bahasa
//! Inggris (pemilihan tool oleh LLM lebih stabil dan bisa dipakai pengguna
//! non-Indonesia); teks hasil mengikuti bahasa isi vault.
//!
//! [`Profile`] memilih subset tool yang diekspos: `full` (bawaan) atau `coding`
//! (hanya tool yang berguna di sesi koding, agar tidak membebani konteks).

use std::time::Duration;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use schemars::JsonSchema;
use schemars::generate::SchemaSettings;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use memory_retrieve::RecallQuery;

use crate::AppState;

/// Versi protokol yang ditawarkan bila klien meminta versi yang tidak dikenal.
pub const PROTOCOL_VERSION: &str = "2026-07-28";
const SUPPORTED: &[&str] = &["2026-07-28", "2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

/// Subset tool yang diekspos ke klien.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Profile {
    /// Semua tool (memori hidup + proyek).
    #[default]
    Full,
    /// Hanya tool untuk sesi koding: konteks proyek, recall, simpan keputusan,
    /// entitas, kronologi, prinsip kerja.
    Coding,
}

/// Tool yang masuk profil `coding`, urut sesuai prioritas pemakaian.
pub const CODING_TOOLS: &[&str] = &["project_context", "recall", "get_episodes", "remember", "who_is", "timeline", "principles"];

impl Profile {
    pub const ALL: [Profile; 2] = [Profile::Full, Profile::Coding];

    pub fn parse(s: &str) -> Option<Profile> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "full" | "all" => Some(Profile::Full),
            "coding" | "code" | "dev" => Some(Profile::Coding),
            _ => None,
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Profile::Full => "full",
            Profile::Coding => "coding",
        }
    }

    pub fn includes(self, tool: &str) -> bool {
        match self {
            Profile::Full => true,
            Profile::Coding => CODING_TOOLS.contains(&tool),
        }
    }
}

fn schema(props: Value, required: &[&str]) -> Value {
    json!({"type": "object", "properties": props, "required": required})
}

/// Skema JSON (draft 2020-12, subschema inline, tanpa `$schema`) untuk `outputSchema`.
fn out_schema<T: JsonSchema>() -> Value {
    let mut generator = SchemaSettings::draft2020_12()
        .with(|s| {
            s.inline_subschemas = true;
            s.meta_schema = None;
        })
        .into_generator();
    generator.root_schema_for::<T>().to_value()
}

fn subject_prop() -> Value {
    json!({"type": "string", "description": "Memory subject ID (default: the vault's default subject)"})
}

/// Petunjuk perilaku tool untuk klien (spesifikasi MCP `ToolAnnotations`).
#[derive(Clone, Copy)]
struct Hints {
    read_only: bool,
    destructive: bool,
    idempotent: bool,
}

/// Hanya membaca vault; aman dipanggil tanpa konfirmasi pengguna.
const READ: Hints = Hints { read_only: true, destructive: false, idempotent: true };
/// Menambah data (tidak mengubah/menghapus yang ada); teks sama akan dideteksi duplikat.
const APPEND: Hints = Hints { read_only: false, destructive: false, idempotent: true };
/// Mengubah status fakta/episode yang ada (non-destruktif, tetapi layak dikonfirmasi).
const MUTATE: Hints = Hints { read_only: false, destructive: true, idempotent: false };

fn tool(name: &str, title: &str, description: &str, input: Value, output: Value, h: Hints) -> Value {
    json!({
        "name": name,
        "title": title,
        "description": description,
        "inputSchema": input,
        "outputSchema": output,
        "annotations": {
            "title": title,
            "readOnlyHint": h.read_only,
            "destructiveHint": h.destructive,
            "idempotentHint": h.idempotent,
            // Vault lokal milik pengguna, bukan dunia terbuka (web, API pihak ketiga).
            "openWorldHint": false
        }
    })
}

/// Semua definisi tool (profil `full`).
fn all_tools() -> Vec<Value> {
    let date = |d: &str| json!({"type": "string", "description": format!("{d} (YYYY-MM-DD)")});
    vec![
        tool(
            "project_context",
            "Project context",
            "Get the full cross-tool context of a project, repository, or service before working on it: the project entity, architecture and configuration facts that hold now, technical decisions and bug fixes, the subject's relevant engineering principles, and related past sessions. Call this at the start of a coding task.",
            schema(json!({
                "project": {"type": "string", "description": "Project, repository, or service name (e.g. 'LLM-MEMORY' or 'BACKEND/api-data-warehouse')"},
                "task": {"type": "string", "description": "The specific task or topic being worked on (optional; focuses the result)"},
                "subject": subject_prop(),
                "budget": {"type": "integer", "minimum": 200, "maximum": 50000, "description": "Approximate token budget for the text result (default from config, ~2500)"}
            }), &["project"]),
            out_schema::<memory_retrieve::ProjectContext>(),
            READ,
        ),
        tool(
            "recall",
            "Recall memories",
            "Search the subject's memory (technical decisions, project architecture, bug root causes, stories, facts, work/life principles, traits) with hybrid vector + keyword + graph retrieval. Every fact is labelled 'current' or 'past' with its validity range. Set 'project' to prioritise one project/repository, 'from'/'to' for a time window, and 'as_of' to see memory as it stood on that date. Results come back as a compact index by default; call get_episodes with the IDs you need.",
            schema(json!({
                "query": {"type": "string", "description": "Question, topic, bug, or feature/project name"},
                "project": {"type": "string", "description": "Filter/prioritise a project or service name (e.g. 'LLM-MEMORY' or 'BACKEND/api-master-sensor')"},
                "subject": subject_prop(),
                "from": date("Start of time window"),
                "to": date("End of time window"),
                "as_of": date("View memory as of this date"),
                "kinds": {"type": "array", "items": {"type": "string", "enum": ["episode", "fact", "principle", "disposition"]}, "description": "Restrict result kinds"},
                "limit": {"type": "integer", "minimum": 1, "maximum": 50},
                "detail": {"type": "string", "enum": ["index", "full"], "description": "'index' (default): one line per hit with its ID, date, type and token cost, so you can fetch only what matters with get_episodes; 'full': summaries and fact history inline"},
                "budget": {"type": "integer", "minimum": 200, "maximum": 50000, "description": "Approximate token budget for the text result"}
            }), &["query"]),
            out_schema::<memory_retrieve::RecallResult>(),
            READ,
        ),
        tool(
            "remember",
            "Remember",
            "Save a new technical decision, project architecture note, bug root cause, work preference, or personal story/fact to cross-tool long-term memory. The ingestion pipeline filters small talk, so it is safe to call proactively. Identical text is detected as a duplicate.",
            schema(json!({
                "text": {"type": "string", "description": "What to remember, in the user's own words where possible"},
                "source": {"type": "string", "description": "Origin as '<client>:<project>', e.g. 'claude-code:LLM-MEMORY' or 'codex:api-master-sensor'"},
                "project": {"type": "string", "description": "Related project/repository name (optional; derived from 'source' when it has the '<client>:<project>' form)"},
                "subject": subject_prop()
            }), &["text"]),
            json!({
                "type": "object",
                "properties": {
                    "source_id": {"type": "string", "description": "ID of the stored source document"},
                    "duplicate": {"type": "boolean", "description": "True when identical text was already stored"},
                    "job_id": {"type": ["string", "null"], "description": "Background extraction job, null when nothing new to process"},
                    "note": {"type": "string"}
                },
                "required": ["source_id", "duplicate"]
            }),
            APPEND,
        ),
        tool(
            "who_is",
            "Entity details",
            "Details of an entity in the subject's memory (person, project/repository, component/file, tool/technology, organisation, place, concept): aliases, kinship (for people), facts that still hold, history of changes, and episodes mentioning it.",
            schema(json!({"name": {"type": "string", "description": "Entity name or alias"}, "subject": subject_prop()}), &["name"]),
            out_schema::<memory_retrieve::WhoIs>(),
            READ,
        ),
        tool(
            "timeline",
            "Timeline",
            "Chronological sequence of episodes and fact changes, optionally filtered by entity/project ('entity'), life or project phase ('phase'), or time window. Pass 'anchor' (an episode ID from recall or the session index) to see what happened before and after that memory in the same project.",
            schema(json!({
                "subject": subject_prop(),
                "entity": {"type": "string", "description": "Person, project, component, or place name"},
                "phase": {"type": "string", "description": "Life phase or project phase"},
                "from": date("Start"), "to": date("End"),
                "limit": {"type": "integer", "minimum": 1, "maximum": 500},
                "anchor": {"type": "string", "description": "Episode ID to centre the timeline on (from recall / get_episodes / session index)"},
                "before": {"type": "integer", "minimum": 0, "maximum": 50, "description": "Episodes before the anchor (default 3)"},
                "after": {"type": "integer", "minimum": 0, "maximum": 50, "description": "Episodes after the anchor (default 3)"},
                "project": {"type": "string", "description": "Restrict the anchor neighbourhood to this project (default: the anchor's project)"}
            }), &[]),
            out_schema::<memory_retrieve::Timeline>(),
            READ,
        ),
        tool(
            "get_episodes",
            "Get episodes",
            "Fetch the full details of specific memories by ID (text, summary, decisions, facts, files touched, concepts, lessons, source). Use after recall, timeline, or the session-start index; always batch several IDs in one call. Roughly 300-1000 tokens per episode.",
            schema(json!({
                "ids": {"type": "array", "items": {"type": "string"}, "minItems": 1, "maxItems": 25, "description": "Episode IDs (with or without the 'ep:' prefix)"},
                "subject": subject_prop(),
                "budget": {"type": "integer", "minimum": 200, "maximum": 50000, "description": "Approximate token budget for the text result"}
            }), &["ids"]),
            out_schema::<memory_retrieve::EpisodesDetail>(),
            READ,
        ),
        tool(
            "principles",
            "Principles",
            "The subject's working principles, engineering standards, preferences, and decision patterns, each backed by several real episodes across time and projects, ranked by relevance to the given situation.",
            schema(json!({"situation": {"type": "string", "description": "The situation or decision at hand"}, "subject": subject_prop()}), &[]),
            json!({"type": "object", "properties": {"principles": {"type": "array", "items": out_schema::<memory_retrieve::PrincipleView>()}}, "required": ["principles"]}),
            READ,
        ),
        tool(
            "persona_context",
            "Persona context",
            "A ready-to-use system prompt for answering AS the subject: speaking style, principles, relevant memories, and anti-fabrication rules. Set 'as_of' for the subject's 'version' on a given date.",
            schema(json!({"question": {"type": "string"}, "as_of": date("Reference date"), "subject": subject_prop()}), &["question"]),
            json!({
                "type": "object",
                "properties": {
                    "found": {"type": "boolean"},
                    "age_at_reference": {"type": ["integer", "null"], "description": "Subject's age on the reference date, when known"}
                },
                "required": ["found"]
            }),
            READ,
        ),
        tool(
            "family_tree",
            "Family tree",
            "The subject's family tree: relations told directly and derived relations (grandparents, cousins, in-laws).",
            schema(json!({"subject": subject_prop()}), &[]),
            out_schema::<memory_retrieve::FamilyTree>(),
            READ,
        ),
        tool(
            "memory_gaps",
            "Memory gaps",
            "Gaps in the subject's memory (life phases with few stories, people mentioned only once) to guide the next interview questions.",
            schema(json!({"subject": subject_prop()}), &[]),
            json!({
                "type": "object",
                "properties": {
                    "gaps": {"type": "array", "items": {
                        "type": "object",
                        "properties": {
                            "gap": {"type": "string"},
                            "target": {"type": ["string", "null"]},
                            "description": {"type": "string"},
                            "priority": {"type": "integer"}
                        },
                        "required": ["gap", "description", "priority"]
                    }}
                },
                "required": ["gaps"]
            }),
            READ,
        ),
        tool(
            "list_subjects",
            "List subjects",
            "List the subjects (people) whose memory is stored in this vault.",
            schema(json!({}), &[]),
            json!({"type": "object", "properties": {"subjects": {"type": "array", "items": out_schema::<memory_core::Subject>()}}, "required": ["subjects"]}),
            READ,
        ),
        tool(
            "compact",
            "Compact memory",
            "Summarise and compact memory non-destructively, keeping the original evidence. mode='candidates' lists clusters of similar episodes, overlapping facts per entity, and open reviews; mode='apply' submits a curated plan (episode_rollups mark micro-episodes as retellings of a new canonical episode, fact_merges merge duplicate facts, fact_closures close outdated facts, fact_rejections drop transient states); mode='auto' runs the compaction with the server's own LLM.",
            schema(json!({
                "subject": subject_prop(),
                "mode": {"type": "string", "enum": ["candidates", "apply", "auto"], "description": "Operation mode (default: 'apply' when a plan is given, otherwise 'candidates')"},
                "plan": {
                    "type": "object",
                    "description": "Compaction plan for mode='apply'",
                    "properties": {
                        "episode_rollups": {
                            "type": "array",
                            "items": schema(json!({
                                "episode_ids": {"type": "array", "items": {"type": "string"}},
                                "title": {"type": "string"},
                                "summary": {"type": "string"},
                                "text": {"type": "string"}
                            }), &["episode_ids", "title", "summary", "text"])
                        },
                        "fact_merges": {
                            "type": "array",
                            "items": schema(json!({
                                "keep_id": {"type": "string"},
                                "absorb_ids": {"type": "array", "items": {"type": "string"}},
                                "predicate": {"type": "string"},
                                "object_text": {"type": "string"}
                            }), &["keep_id", "absorb_ids"])
                        },
                        "fact_closures": {
                            "type": "array",
                            "items": schema(json!({
                                "fact_id": {"type": "string"},
                                "valid_to": {"type": "string"},
                                "superseded_by": {"type": "string"}
                            }), &["fact_id"])
                        },
                        "fact_rejections": {"type": "array", "items": {"type": "string"}},
                        "review_decisions": {
                            "type": "array",
                            "items": schema(json!({
                                "review_id": {"type": "string"},
                                "decision": {"type": "string", "enum": ["approve", "reject", "keep_both"]},
                                "note": {"type": "string"}
                            }), &["review_id", "decision"])
                        }
                    }
                }
            }), &[]),
            json!({
                "type": "object",
                "properties": {
                    "mode": {"type": "string", "enum": ["candidates", "apply", "auto"]},
                    "candidates": out_schema::<memory_extract::compact::CompactCandidates>(),
                    "report": out_schema::<memory_extract::compact::CompactReport>()
                },
                "required": ["mode"]
            }),
            MUTATE,
        ),
    ]
}

/// Daftar tool untuk `tools/list` sesuai profil.
pub fn tools(profile: Profile) -> Value {
    let mut v: Vec<Value> = all_tools().into_iter().filter(|t| profile.includes(t["name"].as_str().unwrap_or(""))).collect();
    if profile == Profile::Coding {
        // Urutkan sesuai prioritas pemakaian di sesi koding.
        v.sort_by_key(|t| CODING_TOOLS.iter().position(|n| Some(*n) == t["name"].as_str()).unwrap_or(usize::MAX));
    }
    Value::Array(v)
}

fn text_result(text: String, structured: Option<Value>) -> Value {
    let mut r = json!({"content": [{"type": "text", "text": text}], "isError": false});
    if let Some(s) = structured {
        r["structuredContent"] = s;
    }
    r
}

/// Anggaran token teks dari argumen `budget` (bawaan `retrieval.tool_budget_tokens`).
fn budget_tokens(st: &AppState, args: &Value) -> usize {
    args["budget"].as_u64().map(|b| b as usize).unwrap_or(st.rt.cfg.retrieval.tool_budget_tokens).clamp(200, 50_000)
}

/// Semua tool lewat sini: teks hasil dipotong ke anggaran (pagar terakhir), sehingga
/// tidak ada tool yang membanjiri konteks klien.
async fn call_tool(st: &AppState, name: &str, args: &Value) -> Result<Value, String> {
    let mut v = call_tool_inner(st, name, args).await?;
    let max_chars = budget_tokens(st, args) * 4 + 400;
    if let Some(text) = v["content"][0]["text"].as_str()
        && text.chars().count() > max_chars
    {
        let cut = memory_retrieve::fit_text(text, max_chars);
        v["content"][0]["text"] = json!(cut);
    }
    Ok(v)
}

async fn call_tool_inner(st: &AppState, name: &str, args: &Value) -> Result<Value, String> {
    let subject = st.resolve_subject(args["subject"].as_str()).await;
    if !memory_core::ids::is_valid_id(&subject) {
        return Err(format!("subjek tidak valid: {subject}"));
    }
    let s = |k: &str| args[k].as_str().filter(|v| !v.is_empty()).map(str::to_string);
    let r = &st.rt.retriever;
    let e = |e: memory_retrieve::RetrieveError| e.to_string();
    match name {
        "recall" => {
            let q = RecallQuery {
                subject,
                query: s("query").ok_or("query wajib diisi")?,
                project: s("project"),
                from: s("from"),
                to: s("to"),
                as_of: s("as_of"),
                kinds: args["kinds"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default(),
                limit: args["limit"].as_u64().map(|x| x as usize),
                // Agent: indeks ringkas dulu, detail per ID lewat get_episodes.
                detail: Some(s("detail").unwrap_or_else(|| "index".into())),
                budget: args["budget"].as_u64().map(|x| x as usize),
            };
            let mut res = r.recall(&q).await.map_err(e)?;
            // structuredContent ikut diringkas di mode indeks (kutipan panjang dibuang).
            if q.index_mode() {
                for ep in &mut res.episodes {
                    ep.excerpt = memory_core::text::truncate_chars(&ep.excerpt, 160);
                    ep.summary = memory_core::text::truncate_chars(&ep.summary, 200);
                }
            }
            Ok(text_result(res.context.clone(), Some(json!(res))))
        }
        "project_context" => {
            let project = s("project").ok_or("project wajib diisi")?;
            let opts = memory_retrieve::ToolOpts { budget_tokens: Some(budget_tokens(st, args)) };
            let pc = r.project_context_with(&subject, &project, s("task").as_deref(), &opts).await.map_err(e)?;
            let slim = json!({
                "project": pc.project, "found": pc.found,
                "entity": pc.entity.as_ref().map(|en| json!({"id": en.id, "name": en.name, "aliases": en.aliases})),
                "current_facts": pc.current_facts.iter().map(|f| json!({"id": f.id, "statement": f.statement, "category": f.category, "valid_from": f.valid_from, "evidence": f.evidence})).collect::<Vec<_>>(),
                "decisions": pc.decisions,
                "lessons": pc.lessons.iter().map(|l| json!({"id": l.id, "kind": l.kind, "title": l.title, "occurrences": l.occurrences})).collect::<Vec<_>>(),
                "principles": pc.principles.iter().map(|p| json!({"id": p.principle.id, "statement": p.principle.statement, "confidence": p.principle.confidence})).collect::<Vec<_>>(),
                "episodes": pc.episodes.iter().map(|ep| json!({"id": ep.id, "title": ep.title, "time": ep.time, "obs_type": ep.obs_type, "approx_tokens": ep.approx_tokens})).collect::<Vec<_>>(),
            });
            Ok(text_result(pc.context.clone(), Some(slim)))
        }
        "get_episodes" => {
            let ids: Vec<String> = args["ids"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default();
            if ids.is_empty() {
                return Err("ids wajib diisi".into());
            }
            let d = r.get_episodes(&subject, &ids, budget_tokens(st, args)).await.map_err(e)?;
            Ok(text_result(d.context.clone(), Some(json!(d))))
        }
        "who_is" => {
            let name = s("name").ok_or("name wajib diisi")?;
            match r.who_is(&subject, &name).await.map_err(e)? {
                Some(w) => {
                    let mut t = format!("{} ({})", w.entity.name, w.entity.kind.label_id());
                    if let Some(rel) = &w.relation_to_subject {
                        t.push_str(&format!(", {rel} dari subjek"));
                    }
                    if !w.entity.aliases.is_empty() {
                        t.push_str(&format!("\nAlias: {}", w.entity.aliases.join(", ")));
                    }
                    for f in &w.current_facts {
                        t.push_str(&format!("\n- {} (berlaku sekarang)", f.statement));
                    }
                    for f in &w.history {
                        t.push_str(&format!("\n- {} (dulu, {}–{})", f.statement, f.valid_from.as_deref().unwrap_or("?"), f.valid_to.as_deref().unwrap_or("?")));
                    }
                    for k in &w.kin {
                        t.push_str(&format!("\n- {}: {}{}", k.relation, k.name, if k.inferred { " (turunan)" } else { "" }));
                    }
                    for ep in w.episodes.iter().take(10) {
                        t.push_str(&format!("\n- [ep:{}] ({}) {}", ep.id, ep.time, ep.title));
                    }
                    Ok(text_result(t, Some(json!(w))))
                }
                None => Ok(text_result(format!("Tidak ada catatan tentang '{name}'."), None)),
            }
        }
        "timeline" => {
            let tl = match s("anchor") {
                Some(anchor) => {
                    let before = args["before"].as_u64().unwrap_or(3) as usize;
                    let after = args["after"].as_u64().unwrap_or(3) as usize;
                    r.timeline_around(&subject, &anchor, before, after, s("project").as_deref()).await.map_err(e)?
                }
                None => r
                    .timeline(&subject, s("entity").as_deref(), s("phase").as_deref(), s("from").as_deref(), s("to").as_deref(), args["limit"].as_u64().unwrap_or(100) as usize)
                    .await
                    .map_err(e)?,
            };
            let t = tl
                .items
                .iter()
                .map(|i| {
                    if i.kind == "episode" {
                        let line = memory_retrieve::index_line(&i.id, i.date.as_deref().unwrap_or("?"), i.obs_type.as_deref(), i.project.as_deref(), &i.label, i.approx_tokens);
                        if i.anchor { format!("{line} <- jangkar") } else { line }
                    } else {
                        format!("- {} [{}] {} ({})", i.date.as_deref().unwrap_or("?"), i.kind, i.label, i.id)
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            Ok(text_result(if t.is_empty() { "Tidak ada catatan.".into() } else { t }, Some(json!(tl))))
        }
        "principles" => {
            let ps = r.principles(&subject, s("situation").as_deref()).await.map_err(e)?;
            let t = ps
                .iter()
                .take(10)
                .map(|p| format!("- {} (keyakinan {:.2}; bukti: {})", p.principle.statement, p.principle.confidence, p.evidence.iter().map(|e| format!("[ep:{}]", e.id)).collect::<Vec<_>>().join("")))
                .collect::<Vec<_>>()
                .join("\n");
            Ok(text_result(if t.is_empty() { "Belum ada prinsip yang cukup bukti.".into() } else { t }, Some(json!({"principles": ps}))))
        }
        "persona_context" => {
            let pc = r.persona_context(&subject, &s("question").ok_or("question wajib diisi")?, s("as_of").as_deref()).await.map_err(e)?;
            Ok(text_result(pc.system_prompt.clone(), Some(json!({"found": pc.found, "age_at_reference": pc.age_at_reference}))))
        }
        "family_tree" => {
            let tree = r.family_tree(&subject).await.map_err(e)?;
            let names: std::collections::HashMap<&str, &str> = tree.nodes.iter().map(|n| (n.id.as_str(), n.name.as_str())).collect();
            let self_id = tree.nodes.iter().find(|n| n.is_self).map(|n| n.id.as_str());
            let t = tree
                .edges
                .iter()
                .filter(|e| Some(e.to.as_str()) == self_id)
                .map(|e| format!("- {}: {}{}", e.label, names.get(e.from.as_str()).unwrap_or(&"?"), if e.inferred { " (turunan)" } else { "" }))
                .collect::<Vec<_>>()
                .join("\n");
            Ok(text_result(if t.is_empty() { "Belum ada data kekerabatan.".into() } else { t }, Some(json!(tree))))
        }
        "remember" => {
            let v = crate::api::remember_text_with_project(st, &subject, &s("text").ok_or("text wajib diisi")?, s("source").as_deref(), s("project").as_deref()).await.map_err(|e| e.to_string())?;
            Ok(text_result("Tersimpan; akan diproses pipeline memori.".into(), Some(v)))
        }
        "memory_gaps" => {
            let gaps = st.rt.pipeline.memory_gaps(&subject).await.map_err(|e| e.to_string())?;
            let t = gaps.iter().take(10).map(|g| format!("- {}", g.description)).collect::<Vec<_>>().join("\n");
            Ok(text_result(t, Some(json!({"gaps": gaps}))))
        }
        "list_subjects" => {
            let subs = st.rt.store.list_subjects().await.map_err(|e| e.to_string())?;
            let t = subs.iter().map(|s| format!("- {} ({})", s.name, s.id)).collect::<Vec<_>>().join("\n");
            Ok(text_result(t, Some(json!({"subjects": subs}))))
        }
        "compact" => {
            let has_plan = args.get("plan").is_some_and(|v| !v.is_null());
            let mode = s("mode").unwrap_or_else(|| if has_plan { "apply".into() } else { "candidates".into() });
            match mode.as_str() {
                "candidates" => {
                    let c = st.rt.pipeline.compact_candidates(&subject).await.map_err(|e| e.to_string())?;
                    let mut out = format!(
                        "Kandidat pemadatan subjek '{subject}': {} klaster episode, {} grup fakta entitas, {} review terbuka.\n",
                        c.episode_clusters.len(),
                        c.entity_fact_groups.len(),
                        c.open_reviews.len()
                    );
                    for (i, cl) in c.episode_clusters.iter().enumerate() {
                        out.push_str(&format!("\n## Klaster Episode #{} ({})\n", i + 1, cl.reason));
                        for ep in &cl.episodes {
                            out.push_str(&format!("- [{}] ({} | {}) {}: {}\n", ep.id, ep.time, ep.source_title, ep.title, ep.summary));
                        }
                    }
                    for eg in &c.entity_fact_groups {
                        out.push_str(&format!("\n## Fakta Aktif: {} ({})\n", eg.entity_name, eg.entity_id));
                        for f in &eg.facts {
                            out.push_str(&format!(
                                "- [{}] ({}, sejak {}, {} bukti) {} {} {}\n",
                                f.id,
                                if f.category.is_empty() { "umum" } else { &f.category },
                                f.valid_from.as_deref().unwrap_or("?"),
                                f.evidence_count,
                                eg.entity_name,
                                f.predicate,
                                f.object_text
                            ));
                        }
                    }
                    if !c.open_reviews.is_empty() {
                        out.push_str("\n## Review Terbuka\n");
                        for rev in &c.open_reviews {
                            out.push_str(&format!("- [{}] ({:?}) {}\n", rev.id, rev.kind, rev.summary));
                        }
                    }
                    Ok(text_result(out, Some(json!({"mode": "candidates", "candidates": c}))))
                }
                "apply" => {
                    let plan_val = args.get("plan").cloned().unwrap_or_else(|| json!({}));
                    let plan: memory_extract::compact::CompactPlan =
                        serde_json::from_value(plan_val).map_err(|e| format!("format plan tidak valid: {e}"))?;
                    let rep = st.rt.pipeline.compact_apply(&subject, plan).await.map_err(|e| e.to_string())?;
                    let msg = format!(
                        "Pemadatan diterapkan: {} episode kanonik dibuat (dari {} episode mikro), {} fakta digabung, {} fakta ditutup, {} fakta sesaat ditolak, {} review diselesaikan.",
                        rep.canonical_episodes_created,
                        rep.episodes_compacted,
                        rep.facts_merged,
                        rep.facts_closed,
                        rep.facts_rejected,
                        rep.reviews_resolved
                    );
                    Ok(text_result(msg, Some(json!({"mode": "apply", "report": rep}))))
                }
                "auto" => {
                    let rep = st.rt.pipeline.compact(&subject).await.map_err(|e| e.to_string())?;
                    let msg = format!(
                        "Pemadatan otomatis selesai ({} klaster diperiksa): {} episode kanonik dibuat (dari {} episode mikro), {} fakta digabung, {} fakta ditutup, {} fakta sesaat ditolak.",
                        rep.episode_clusters,
                        rep.canonical_episodes_created,
                        rep.episodes_compacted,
                        rep.facts_merged,
                        rep.facts_closed,
                        rep.facts_rejected
                    );
                    Ok(text_result(msg, Some(json!({"mode": "auto", "report": rep}))))
                }
                other => Err(format!("mode compact tidak dikenal: {other} (gunakan 'candidates', 'apply', atau 'auto')")),
            }
        }
        other => Err(format!("tool tidak dikenal: {other}")),
    }
}

fn rpc_error(id: Value, code: i64, msg: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": msg}})
}

/// Instruksi server yang dikirim saat `initialize` (masuk ke system prompt klien).
const INSTRUCTIONS: &str = "Engram is the user's cross-tool long-term memory: coding projects, architecture decisions, \
bug root causes, work principles, and personal life. Before answering about a project or the user's past, call \
project_context (at the start of a coding task) or recall/who_is/timeline. Facts are labelled current vs past: never \
present a past fact as current. When a tool says there is no record, say so instead of guessing. Save important \
decisions and context with remember, using source='<client>:<project>'.";

/// Tangani satu pesan JSON-RPC. `None` untuk notifikasi.
pub async fn handle(st: &AppState, profile: Profile, msg: Value) -> Option<Value> {
    let id = msg.get("id").cloned();
    let method = msg["method"].as_str().unwrap_or("");
    let Some(id) = id else {
        return None; // notifikasi (notifications/initialized, dll.)
    };
    let result = match method {
        "initialize" => {
            let requested = msg["params"]["protocolVersion"].as_str().unwrap_or(PROTOCOL_VERSION);
            let version = if SUPPORTED.contains(&requested) { requested } else { PROTOCOL_VERSION };
            json!({
                "protocolVersion": version,
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {
                    "name": "engram",
                    "title": "Engram Memory",
                    "version": env!("CARGO_PKG_VERSION"),
                    "websiteUrl": "https://github.com/cmjcode/engram"
                },
                "instructions": INSTRUCTIONS
            })
        }
        "ping" => json!({}),
        "tools/list" => json!({"tools": tools(profile)}),
        "tools/call" => {
            let name = msg["params"]["name"].as_str().unwrap_or("");
            let args = msg["params"].get("arguments").cloned().unwrap_or(json!({}));
            match call_tool(st, name, &args).await {
                Ok(v) => v,
                Err(e) => json!({"content": [{"type": "text", "text": e}], "isError": true}),
            }
        }
        "" => return Some(rpc_error(id, -32600, "permintaan tidak valid")),
        _ => return Some(rpc_error(id, -32601, &format!("metode tidak dikenal: {method}"))),
    };
    Some(json!({"jsonrpc": "2.0", "id": id, "result": result}))
}

/// Parameter query `POST /mcp?profile=coding`.
#[derive(Debug, Default, serde::Deserialize)]
pub struct McpQuery {
    #[serde(default)]
    pub profile: Option<String>,
}

fn bad_request(msg: &str) -> Response {
    (StatusCode::BAD_REQUEST, Json(rpc_error(Value::Null, -32600, msg))).into_response()
}

/// Streamable HTTP: `POST /mcp`. Satu pesan atau batch; respons JSON
/// (`202 Accepted` untuk notifikasi saja).
pub async fn http_handler(State(st): State<AppState>, Query(q): Query<McpQuery>, Json(body): Json<Value>) -> Response {
    let Some(profile) = Profile::parse(q.profile.as_deref().unwrap_or("")) else {
        return bad_request("profile tidak dikenal (pakai 'full' atau 'coding')");
    };
    match body {
        Value::Array(items) => {
            let mut out = Vec::new();
            for m in items {
                if let Some(r) = handle(&st, profile, m).await {
                    out.push(r);
                }
            }
            if out.is_empty() { StatusCode::ACCEPTED.into_response() } else { Json(Value::Array(out)).into_response() }
        }
        m => match handle(&st, profile, m).await {
            Some(r) => Json(r).into_response(),
            None => StatusCode::ACCEPTED.into_response(),
        },
    }
}

/// Streamable HTTP: `GET /mcp` membuka stream SSE untuk pesan server→klien.
/// Server ini tidak pernah mengirim pesan sendiri, jadi stream hanya berisi
/// keep-alive; klien yang tidak meminta `text/event-stream` mendapat 405.
pub async fn http_get(headers: HeaderMap) -> Response {
    let wants_sse = headers.get(header::ACCEPT).and_then(|v| v.to_str().ok()).is_some_and(|a| a.contains("text/event-stream"));
    if !wants_sse {
        return (StatusCode::METHOD_NOT_ALLOWED, [(header::ALLOW, "POST")], "GET /mcp memerlukan Accept: text/event-stream").into_response();
    }
    let stream = futures::stream::pending::<Result<Event, std::convert::Infallible>>();
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)).text("keep-alive")).into_response()
}

/// Transport stdio: satu pesan JSON per baris di stdin/stdout. Log ke stderr.
pub async fn serve_stdio(st: AppState, profile: Profile) -> std::io::Result<()> {
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut out = tokio::io::stdout();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let resp = match serde_json::from_str::<Value>(&line) {
            Ok(Value::Array(items)) => {
                let mut v = Vec::new();
                for m in items {
                    v.extend(handle(&st, profile, m).await);
                }
                (!v.is_empty()).then_some(Value::Array(v))
            }
            Ok(m) => handle(&st, profile, m).await,
            Err(e) => Some(rpc_error(Value::Null, -32700, &format!("JSON tidak valid: {e}"))),
        };
        if let Some(r) = resp {
            out.write_all(serde_json::to_string(&r).unwrap_or_default().as_bytes()).await?;
            out.write_all(b"\n").await?;
            out.flush().await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_tool_declares_title_annotations_and_schemas() {
        let tools = all_tools();
        assert_eq!(tools.len(), 12);
        for t in &tools {
            let name = t["name"].as_str().unwrap();
            assert!(t["title"].as_str().is_some_and(|s| !s.is_empty()), "{name}: title");
            assert!(t["description"].as_str().is_some_and(|s| s.is_ascii()), "{name}: deskripsi harus Inggris/ASCII");
            assert_eq!(t["inputSchema"]["type"], "object", "{name}: inputSchema");
            assert_eq!(t["outputSchema"]["type"], "object", "{name}: outputSchema harus objek");
            // Subschema harus inline: tidak ada $ref/$defs/$schema yang menyulitkan validator klien.
            let dump = t["outputSchema"].to_string();
            assert!(!dump.contains("\"$ref\"") && !dump.contains("\"$defs\"") && !dump.contains("\"$schema\""), "{name}: {dump}");
            let a = &t["annotations"];
            for k in ["readOnlyHint", "destructiveHint", "idempotentHint", "openWorldHint"] {
                assert!(a[k].is_boolean(), "{name}: annotations.{k}");
            }
            assert_eq!(a["openWorldHint"], false, "{name}");
        }
        let hints = |n: &str| tools.iter().find(|t| t["name"] == n).unwrap()["annotations"].clone();
        assert_eq!(hints("recall")["readOnlyHint"], true);
        assert_eq!(hints("remember")["readOnlyHint"], false);
        assert_eq!(hints("remember")["destructiveHint"], false);
        assert_eq!(hints("compact")["destructiveHint"], true);
    }

    #[test]
    fn coding_profile_exposes_only_coding_tools_in_priority_order() {
        let names: Vec<String> = tools(Profile::Coding).as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect();
        assert_eq!(names, CODING_TOOLS);
        assert_eq!(tools(Profile::Full).as_array().unwrap().len(), 12);
        assert_eq!(Profile::parse("coding"), Some(Profile::Coding));
        assert_eq!(Profile::parse(""), Some(Profile::Full));
        assert_eq!(Profile::parse("nope"), None);
    }

    #[test]
    fn protocol_versions() {
        assert_eq!(PROTOCOL_VERSION, "2026-07-28");
        for v in ["2024-11-05", "2025-03-26", "2025-06-18", "2025-11-25", "2026-07-28"] {
            assert!(SUPPORTED.contains(&v), "{v}");
        }
    }
}
