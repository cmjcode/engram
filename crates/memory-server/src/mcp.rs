//! Server MCP (Model Context Protocol): agent AI mengambil memori lewat tool.
//!
//! Transport: stdio (`memory mcp`, satu pesan JSON per baris) dan HTTP
//! (`POST /mcp`, respons JSON). Logika temporal & guardrail ada di sisi
//! server, sehingga LLM klien mana pun mendapat konteks yang sama jujurnya.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use memory_retrieve::RecallQuery;

use crate::AppState;

pub const PROTOCOL_VERSION: &str = "2025-06-18";
const SUPPORTED: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];

fn schema(props: Value, required: &[&str]) -> Value {
    json!({"type": "object", "properties": props, "required": required})
}

fn subject_prop() -> Value {
    json!({"type": "string", "description": "ID subjek memori (default: subjek default vault)"})
}

pub fn tools() -> Value {
    let date = |d: &str| json!({"type": "string", "description": format!("{d} (YYYY-MM-DD)")});
    json!([
        {
            "name": "recall",
            "description": "Cari memori hidup subjek (cerita, fakta, prinsip, sifat) dengan pencarian hybrid vektor+kata kunci+graph. Fakta diberi status 'berlaku sekarang' atau 'dulu' dengan rentang waktunya. Pakai from/to untuk periode tertentu, as_of untuk melihat memori sebagaimana pada tanggal itu.",
            "inputSchema": schema(json!({
                "query": {"type": "string", "description": "Pertanyaan atau topik"},
                "subject": subject_prop(),
                "from": date("Awal jendela waktu"),
                "to": date("Akhir jendela waktu"),
                "as_of": date("Lihat memori per tanggal ini"),
                "kinds": {"type": "array", "items": {"type": "string", "enum": ["episode", "fact", "principle", "disposition"]}},
                "limit": {"type": "integer", "minimum": 1, "maximum": 50}
            }), &["query"])
        },
        {
            "name": "who_is",
            "description": "Identitas seseorang/tempat/organisasi dalam hidup subjek: alias, hubungan kekerabatan dengan subjek (pakde, bulik, sepupu, ...), fakta yang masih berlaku, riwayat, dan cerita yang menyebutnya.",
            "inputSchema": schema(json!({"name": {"type": "string"}, "subject": subject_prop()}), &["name"])
        },
        {
            "name": "timeline",
            "description": "Urutan kronologis cerita dan perubahan fakta, bisa disaring per orang (entity), fase hidup (phase), atau periode.",
            "inputSchema": schema(json!({
                "subject": subject_prop(),
                "entity": {"type": "string", "description": "Nama orang/tempat"},
                "phase": {"type": "string", "description": "Fase hidup, mis. 'masa kecil', 'dewasa muda'"},
                "from": date("Awal"), "to": date("Akhir"),
                "limit": {"type": "integer", "minimum": 1, "maximum": 500}
            }), &[])
        },
        {
            "name": "principles",
            "description": "Prinsip hidup / pola keputusan subjek yang didukung beberapa cerita nyata, diurutkan menurut relevansi dengan situasi yang diberikan.",
            "inputSchema": schema(json!({"situation": {"type": "string"}, "subject": subject_prop()}), &[])
        },
        {
            "name": "persona_context",
            "description": "System prompt siap pakai untuk menjawab SEBAGAI subjek: gaya bertutur, prinsip, memori relevan, dan aturan anti-mengarang. Isi as_of untuk 'versi dirinya' pada tanggal tertentu.",
            "inputSchema": schema(json!({"question": {"type": "string"}, "as_of": date("Tanggal acuan"), "subject": subject_prop()}), &["question"])
        },
        {
            "name": "family_tree",
            "description": "Silsilah keluarga subjek: relasi yang diceritakan langsung dan relasi turunan (kakek, sepupu, mertua, ipar).",
            "inputSchema": schema(json!({"subject": subject_prop()}), &[])
        },
        {
            "name": "remember",
            "description": "Simpan percakapan/cerita baru ke memori. Disaring pipeline: basa-basi tidak akan menjadi memori jangka panjang.",
            "inputSchema": schema(json!({"text": {"type": "string"}, "source": {"type": "string", "description": "Asal teks, mis. nama aplikasi"}, "subject": subject_prop()}), &["text"])
        },
        {
            "name": "memory_gaps",
            "description": "Celah memori (fase hidup yang ceritanya sedikit, orang yang baru disebut sekali) untuk memandu pertanyaan wawancara berikutnya.",
            "inputSchema": schema(json!({"subject": subject_prop()}), &[])
        },
        {
            "name": "list_subjects",
            "description": "Daftar subjek (orang) yang memorinya tersimpan di vault ini.",
            "inputSchema": schema(json!({}), &[])
        }
    ])
}

fn text_result(text: String, structured: Option<Value>) -> Value {
    let mut r = json!({"content": [{"type": "text", "text": text}], "isError": false});
    if let Some(s) = structured {
        r["structuredContent"] = s;
    }
    r
}

async fn call_tool(st: &AppState, name: &str, args: &Value) -> Result<Value, String> {
    let subject = args["subject"].as_str().filter(|s| !s.is_empty()).map(str::to_string).unwrap_or_else(|| st.default_subject());
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
                from: s("from"),
                to: s("to"),
                as_of: s("as_of"),
                kinds: args["kinds"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default(),
                limit: args["limit"].as_u64().map(|x| x as usize),
            };
            let res = r.recall(&q).await.map_err(e)?;
            Ok(text_result(res.context.clone(), Some(json!(res))))
        }
        "who_is" => {
            let name = s("name").ok_or("name wajib diisi")?;
            match r.who_is(&subject, &name).await.map_err(e)? {
                Some(w) => {
                    let mut t = format!("{} ({:?})", w.entity.name, w.entity.kind);
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
            let tl = r
                .timeline(&subject, s("entity").as_deref(), s("phase").as_deref(), s("from").as_deref(), s("to").as_deref(), args["limit"].as_u64().unwrap_or(100) as usize)
                .await
                .map_err(e)?;
            let t = tl.items.iter().map(|i| format!("- {} [{}] {} ({})", i.date.as_deref().unwrap_or("?"), i.kind, i.label, i.id)).collect::<Vec<_>>().join("\n");
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
            Ok(text_result(if t.is_empty() { "Belum ada prinsip yang cukup bukti.".into() } else { t }, Some(json!(ps))))
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
            let v = crate::api::remember_text(st, &subject, &s("text").ok_or("text wajib diisi")?, s("source").as_deref()).await.map_err(|e| e.to_string())?;
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
        other => Err(format!("tool tidak dikenal: {other}")),
    }
}

fn rpc_error(id: Value, code: i64, msg: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": msg}})
}

/// Tangani satu pesan JSON-RPC. `None` untuk notifikasi.
pub async fn handle(st: &AppState, msg: Value) -> Option<Value> {
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
                "serverInfo": {"name": "llm-memory", "version": env!("CARGO_PKG_VERSION")},
                "instructions": "Memori hidup seseorang. Panggil recall/who_is/timeline sebelum menjawab tentang hidupnya; selalu bedakan fakta yang masih berlaku dan riwayat; bila tool menjawab 'tidak ada catatan', jangan mengarang. Untuk berbicara sebagai orang itu, pakai persona_context."
            })
        }
        "ping" => json!({}),
        "tools/list" => json!({"tools": tools()}),
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

pub async fn http_handler(State(st): State<AppState>, Json(body): Json<Value>) -> Response {
    match body {
        Value::Array(items) => {
            let mut out = Vec::new();
            for m in items {
                if let Some(r) = handle(&st, m).await {
                    out.push(r);
                }
            }
            if out.is_empty() { StatusCode::ACCEPTED.into_response() } else { Json(Value::Array(out)).into_response() }
        }
        m => match handle(&st, m).await {
            Some(r) => Json(r).into_response(),
            None => StatusCode::ACCEPTED.into_response(),
        },
    }
}

/// Transport stdio: satu pesan JSON per baris di stdin/stdout. Log ke stderr.
pub async fn serve_stdio(st: AppState) -> std::io::Result<()> {
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
                    v.extend(handle(&st, m).await);
                }
                (!v.is_empty()).then_some(Value::Array(v))
            }
            Ok(m) => handle(&st, m).await,
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
