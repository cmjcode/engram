//! Ekspor Telegram Desktop (`result.json`): satu chat atau seluruh akun.

use serde_json::Value;

use memory_core::model::SourceKind;

use crate::{ImportError, Imported, Result};

fn text_of(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .map(|p| match p {
                Value::String(s) => s.clone(),
                other => other["text"].as_str().unwrap_or("").to_string(),
            })
            .collect(),
        _ => String::new(),
    }
}

fn chat_lines(chat: &Value) -> Vec<(String, String)> {
    chat["messages"]
        .as_array()
        .map(|msgs| {
            msgs.iter()
                .filter(|m| m["type"].as_str().unwrap_or("message") == "message")
                .filter_map(|m| {
                    let date = m["date"].as_str()?;
                    let text = text_of(&m["text"]);
                    if text.trim().is_empty() || date.len() < 16 {
                        return None;
                    }
                    let from = m["from"].as_str().unwrap_or("?");
                    let stamp = format!("{} {}", &date[..10], &date[11..16]);
                    Some((stamp.clone(), format!("[{stamp}] {from}: {}", text.replace('\n', " "))))
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn parse(bytes: &[u8], file_name: &str) -> Result<Vec<Imported>> {
    let v: Value = serde_json::from_slice(bytes).map_err(|e| ImportError::Parse(format!("{file_name}: {e}")))?;
    let chats: Vec<&Value> = match v["chats"]["list"].as_array() {
        Some(list) => list.iter().collect(),
        None => vec![&v],
    };
    let mut out = Vec::new();
    for chat in chats {
        let name = chat["name"].as_str().unwrap_or("Telegram");
        out.extend(super::group_chat_by_month(chat_lines(chat), &format!("Telegram: {name}"), SourceKind::Chat, "telegram"));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_chat_with_rich_text() {
        let j = serde_json::json!({"name": "Keluarga", "messages": [
            {"type": "service", "date": "2021-01-01T10:00:00", "action": "create_group"},
            {"type": "message", "date": "2021-01-01T10:01:00", "from": "Budi", "text": ["Lihat ", {"type": "link", "text": "foto lama"}, " kita"]},
            {"type": "message", "date": "2021-02-01T10:01:00", "from": "Ani", "text": "Iya"}
        ]});
        let docs = parse(j.to_string().as_bytes(), "result.json").unwrap();
        assert_eq!(docs.len(), 2);
        assert_eq!(docs[0].text, "[2021-01-01 10:01] Budi: Lihat foto lama kita");
    }
}
