//! Migrasi skema berversi. Setiap migrasi idempoten (`IF NOT EXISTS`) dan
//! tercatat di `_meta:schema`. Dimensi vektor dicatat agar pergantian model
//! embedding tanpa re-index terdeteksi.

use serde_json::json;

use crate::{Result, Store, StoreError, binds};

fn v1(dim: usize, vector: &str, m: u32, efc: u32) -> String {
    let vec_index = |name: &str, table: &str| match vector {
        "diskann" => format!("DEFINE INDEX IF NOT EXISTS {name} ON {table} FIELDS embedding DISKANN DIMENSION {dim} DIST COSINE TYPE F32;"),
        _ => format!("DEFINE INDEX IF NOT EXISTS {name} ON {table} FIELDS embedding HNSW DIMENSION {dim} DIST COSINE TYPE F32 EFC {efc} M {m};"),
    };
    format!(
        r#"
DEFINE ANALYZER IF NOT EXISTS memory_text TOKENIZERS blank, class, punct FILTERS lowercase, ascii;

DEFINE TABLE IF NOT EXISTS subject SCHEMALESS;

DEFINE TABLE IF NOT EXISTS source SCHEMALESS;
DEFINE INDEX IF NOT EXISTS source_hash ON source FIELDS subject, hash UNIQUE;
DEFINE INDEX IF NOT EXISTS source_status ON source FIELDS subject, status;
DEFINE INDEX IF NOT EXISTS source_batch ON source FIELDS batch_id;

DEFINE TABLE IF NOT EXISTS stream_item SCHEMALESS;
DEFINE INDEX IF NOT EXISTS stream_subject ON stream_item FIELDS subject, expires_at;

DEFINE TABLE IF NOT EXISTS episode SCHEMALESS;
DEFINE INDEX IF NOT EXISTS episode_subject ON episode FIELDS subject, status;
DEFINE INDEX IF NOT EXISTS episode_source ON episode FIELDS source_id;
DEFINE INDEX IF NOT EXISTS episode_time ON episode FIELDS subject, time_start;
DEFINE INDEX IF NOT EXISTS episode_entities ON episode FIELDS entity_ids;
DEFINE INDEX IF NOT EXISTS episode_fts ON episode FIELDS text FULLTEXT ANALYZER memory_text BM25;
{episode_vec}

DEFINE TABLE IF NOT EXISTS entity SCHEMALESS;
DEFINE INDEX IF NOT EXISTS entity_subject ON entity FIELDS subject, kind;
DEFINE INDEX IF NOT EXISTS entity_alias ON entity FIELDS alias_keys;
{entity_vec}

DEFINE TABLE IF NOT EXISTS fact SCHEMALESS;
DEFINE INDEX IF NOT EXISTS fact_entity ON fact FIELDS entity_id, category;
DEFINE INDEX IF NOT EXISTS fact_object ON fact FIELDS object_entity;
DEFINE INDEX IF NOT EXISTS fact_subject ON fact FIELDS subject, status;

DEFINE TABLE IF NOT EXISTS kin SCHEMALESS;
DEFINE INDEX IF NOT EXISTS kin_from ON kin FIELDS from;
DEFINE INDEX IF NOT EXISTS kin_to ON kin FIELDS to;
DEFINE INDEX IF NOT EXISTS kin_subject ON kin FIELDS subject;

DEFINE TABLE IF NOT EXISTS principle SCHEMALESS;
DEFINE INDEX IF NOT EXISTS principle_subject ON principle FIELDS subject, status;
DEFINE TABLE IF NOT EXISTS disposition SCHEMALESS;
DEFINE INDEX IF NOT EXISTS disposition_subject ON disposition FIELDS subject, status;
DEFINE TABLE IF NOT EXISTS life_phase SCHEMALESS;
DEFINE INDEX IF NOT EXISTS life_phase_subject ON life_phase FIELDS subject;
DEFINE TABLE IF NOT EXISTS voice_profile SCHEMALESS;

DEFINE TABLE IF NOT EXISTS review_item SCHEMALESS;
DEFINE INDEX IF NOT EXISTS review_status ON review_item FIELDS subject, status, kind;

DEFINE TABLE IF NOT EXISTS import_batch SCHEMALESS;

DEFINE TABLE IF NOT EXISTS job SCHEMALESS;
DEFINE INDEX IF NOT EXISTS job_key ON job FIELDS idempotency_key UNIQUE;
DEFINE INDEX IF NOT EXISTS job_status ON job FIELDS status, run_after;
DEFINE INDEX IF NOT EXISTS job_batch ON job FIELDS batch_id;

DEFINE TABLE IF NOT EXISTS _meta SCHEMALESS;
"#,
        episode_vec = vec_index("episode_vec", "episode"),
        entity_vec = vec_index("entity_vec", "entity"),
    )
}

pub(crate) async fn run(store: &Store) -> Result<()> {
    let meta = store.rows("SELECT * FROM _meta:schema;", binds! {}).await.unwrap_or_default();
    let current = meta.first().and_then(|m| m["version"].as_u64()).unwrap_or(0) as u32;
    if let Some(m) = meta.first() {
        let stored_dim = m["dimension"].as_u64().unwrap_or(0) as usize;
        if stored_dim != 0 && stored_dim != store.dim {
            return Err(StoreError::Schema(format!(
                "vault dibuat dengan dimensi embedding {stored_dim}, konfigurasi sekarang {}. Jalankan `memory reembed` atau kembalikan konfigurasi.",
                store.dim
            )));
        }
    }
    if current < 1 {
        let idx = store.index_config();
        store.execute(&v1(store.dim, &idx.vector, idx.hnsw_m, idx.hnsw_efc), binds! {}).await?;
    }
    if current < memory_core::SCHEMA_VERSION {
        store
            .execute(
                "UPSERT _meta:schema CONTENT $c RETURN NONE;",
                binds! {"c" => json!({
                    "version": memory_core::SCHEMA_VERSION,
                    "dimension": store.dim,
                    "vector_index": store.index_config().vector,
                    "migrated_at": memory_core::ids::now_rfc3339(),
                })},
            )
            .await?;
        tracing::info!(from = current, to = memory_core::SCHEMA_VERSION, "migrasi skema selesai");
    }
    Ok(())
}

/// Hapus dan buat ulang index vektor (setelah ganti model embedding).
pub(crate) async fn rebuild_vector_indexes(store: &Store) -> Result<()> {
    store
        .execute("REMOVE INDEX IF EXISTS episode_vec ON episode; REMOVE INDEX IF EXISTS entity_vec ON entity;", binds! {})
        .await?;
    let idx = store.index_config();
    let ddl = v1(store.dim, &idx.vector, idx.hnsw_m, idx.hnsw_efc);
    let vec_lines: String = ddl.lines().filter(|l| l.contains("_vec ON")).collect::<Vec<_>>().join("\n");
    store.execute(&vec_lines, binds! {}).await?;
    store.execute("UPDATE _meta:schema SET dimension = $d RETURN NONE;", binds! {"d" => store.dim}).await?;
    Ok(())
}
