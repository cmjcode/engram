//! Ekspor percakapan dengan asisten AI (ChatGPT `conversations.json`,
//! Claude `conversations.json`). Satu percakapan = satu dokumen.

use serde_json::Value;

use memory_core::model::SourceKind;

use crate::{ImportError, Imported, Result};

fn epoch_to_iso(v: &Value) -> Option<String> {
    let secs = v.as_f64()?;
    chrono::DateTime::from_timestamp(secs as i64, 0).map(|d| d.format("%Y-%m-%d %H:%M").to_string())
}

fn doc(title: &str, lines: Vec<(String, String)>, format: &str) -> Option<Imported> {
    if lines.is_empty() {
        return None;
    }
    Some(Imported {
        kind: SourceKind::AgentConversation,
        title: format!("{format}: {title}"),
        text: lines.iter().map(|(_, l)| l.as_str()).collect::<Vec<_>>().join("\n"),
        occurred_from: lines.first().map(|(d, _)| d.chars().take(10).collect()),
        occurred_to: lines.last().map(|(d, _)| d.chars().take(10).collect()),
        meta: serde_json::json!({"format": format, "messages": lines.len()}),
    })
}

pub fn parse_chatgpt(bytes: &[u8]) -> Result<Vec<Imported>> {
    let v: Value = serde_json::from_slice(bytes).map_err(|e| ImportError::Parse(e.to_string()))?;
    let convs = v.as_array().cloned().unwrap_or_else(|| vec![v]);
    let mut out = Vec::new();
    for c in &convs {
        let title = c["title"].as_str().unwrap_or("percakapan");
        let mut msgs: Vec<(f64, String, String)> = Vec::new();
        if let Some(map) = c["mapping"].as_object() {
            for node in map.values() {
                let m = &node["message"];
                let role = m["author"]["role"].as_str().unwrap_or("");
                if role != "user" && role != "assistant" {
                    continue;
                }
                let text: String = m["content"]["parts"]
                    .as_array()
                    .map(|p| p.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(" "))
                    .unwrap_or_default();
                if text.trim().is_empty() {
                    continue;
                }
                let t = m["create_time"].as_f64().or(c["create_time"].as_f64()).unwrap_or(0.0);
                msgs.push((t, role.to_string(), text));
            }
        }
        msgs.sort_by(|a, b| a.0.total_cmp(&b.0));
        let lines = msgs
            .into_iter()
            .map(|(t, role, text)| {
                let d = epoch_to_iso(&Value::from(t)).unwrap_or_else(|| "1970-01-01 00:00".into());
                let who = if role == "user" { "Saya" } else { "Asisten" };
                (d.clone(), format!("[{d}] {who}: {}", text.replace('\n', " ")))
            })
            .collect();
        out.extend(doc(title, lines, "ChatGPT"));
    }
    Ok(out)
}

pub fn parse_claude(bytes: &[u8]) -> Result<Vec<Imported>> {
    let v: Value = serde_json::from_slice(bytes).map_err(|e| ImportError::Parse(e.to_string()))?;
    let convs = v.as_array().cloned().unwrap_or_else(|| vec![v]);
    let mut out = Vec::new();
    for c in &convs {
        let title = c["name"].as_str().filter(|s| !s.is_empty()).unwrap_or("percakapan");
        let lines = c["chat_messages"]
            .as_array()
            .map(|msgs| {
                msgs.iter()
                    .filter_map(|m| {
                        let text = m["text"].as_str().map(str::to_string).or_else(|| {
                            m["content"].as_array().map(|parts| parts.iter().filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join(" "))
                        })?;
                        if text.trim().is_empty() {
                            return None;
                        }
                        let d: String = m["created_at"].as_str().unwrap_or("1970-01-01T00:00").replace('T', " ").chars().take(16).collect();
                        let who = if m["sender"].as_str() == Some("human") { "Saya" } else { "Asisten" };
                        Some((d.clone(), format!("[{d}] {who}: {}", text.replace('\n', " "))))
                    })
                    .collect()
            })
            .unwrap_or_default();
        out.extend(doc(title, lines, "Claude"));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chatgpt_and_claude_exports() {
        let g = serde_json::json!([{"title": "Resep", "create_time": 1700000000.0, "mapping": {
            "a": {"message": {"author": {"role": "user"}, "content": {"parts": ["Ibu saya dulu masak gudeg"]}, "create_time": 1700000000.0}},
            "b": {"message": {"author": {"role": "assistant"}, "content": {"parts": ["Menarik!"]}, "create_time": 1700000010.0}},
            "c": {"message": {"author": {"role": "system"}, "content": {"parts": ["x"]}}}
        }}]);
        let d = parse_chatgpt(g.to_string().as_bytes()).unwrap();
        assert_eq!(d.len(), 1);
        assert!(d[0].text.starts_with("[2023-11-14 22:13] Saya: Ibu saya dulu masak gudeg"));
        let c = serde_json::json!([{"name": "Kenangan", "chat_messages": [
            {"sender": "human", "text": "Kakek saya veteran", "created_at": "2024-03-01T10:00:00Z"},
            {"sender": "assistant", "text": "Ceritakan lagi", "created_at": "2024-03-01T10:00:05Z"}
        ]}]);
        let d = parse_claude(c.to_string().as_bytes()).unwrap();
        assert_eq!(d[0].occurred_from.as_deref(), Some("2024-03-01"));
        assert!(d[0].text.contains("Saya: Kakek saya veteran"));
    }
}
