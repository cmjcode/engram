//! Lapisan penyimpanan di atas SurrealDB embedded (RocksDB untuk produksi,
//! memori untuk uji).
//!
//! Semua record disimpan sebagai dokumen JSON dengan ID ULID sebagai kunci
//! record. Relasi (fakta, kekerabatan, sebutan entitas) disimpan sebagai field
//! ID yang di-index, sehingga ekspor ke JSONL tetap sederhana dan portabel.
//! Pencarian vektor lewat trait [`VectorIndex`] agar bisa dipindah ke mesin
//! lain (mis. Qdrant) tanpa mengubah pemanggil.

mod migrations;
pub mod repo;
mod tx;
mod vector;

use std::path::Path;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use surrealdb::Surreal;
use surrealdb::engine::local::{Db, Mem, RocksDb};

use memory_core::config::IndexConfig;

pub use repo::{EpisodeBrief, EpisodeFilter, EpisodeLinkRow, SubjectStats, alias_keys, entity_doc};
pub use tx::Tx;
pub use vector::{SurrealVectorIndex, VectorHit, VectorIndex};

pub const NAMESPACE: &str = "memory";
pub const DATABASE: &str = "main";

/// Semua tabel data (urutan dipakai untuk ekspor/impor).
pub const TABLES: &[&str] = &[
    "subject",
    "source",
    "stream_item",
    "episode",
    "entity",
    "fact",
    "kin",
    "principle",
    "disposition",
    "life_phase",
    "voice_profile",
    "review_item",
    "import_batch",
    "job",
    "session",
    "lesson",
    "project_meta",
];

/// Tabel yang punya field `embedding`.
pub const VECTOR_TABLES: &[&str] = &["episode", "entity"];

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database: {0}")]
    Db(#[from] surrealdb::Error),
    #[error("serde: {0}")]
    Serde(#[from] serde_json::Error),
    #[error("tidak ditemukan: {0}")]
    NotFound(String),
    #[error("skema: {0}")]
    Schema(String),
    #[error("{0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, StoreError>;

impl StoreError {
    /// Konflik transaksi optimistis (dua penulis bersamaan); aman diulang.
    pub fn is_retryable(&self) -> bool {
        matches!(self, StoreError::Db(_)) && self.to_string().contains("can be retried")
    }
}

pub type Binds = Map<String, Value>;

/// `binds!{"s" => subject, "n" => 10}` -> objek variabel query.
#[macro_export]
macro_rules! binds {
    ($($k:expr => $v:expr),* $(,)?) => {{
        #[allow(unused_mut)]
        let mut m = serde_json::Map::new();
        $( m.insert($k.to_string(), serde_json::json!($v)); )*
        m
    }};
}

#[derive(Clone)]
pub struct Store {
    db: Surreal<Db>,
    dim: usize,
    index: IndexConfig,
}

impl Store {
    /// Buka database persisten RocksDB di `dir`.
    pub async fn open(dir: &Path, dim: usize, index: &IndexConfig) -> Result<Self> {
        std::fs::create_dir_all(dir).map_err(|e| StoreError::Invalid(e.to_string()))?;
        let db = Surreal::new::<RocksDb>(dir.to_string_lossy().as_ref()).await?;
        Self::init(db, dim, index).await
    }

    /// Database in-memory (uji).
    pub async fn memory(dim: usize, index: &IndexConfig) -> Result<Self> {
        let db = Surreal::new::<Mem>(()).await?;
        Self::init(db, dim, index).await
    }

    async fn init(db: Surreal<Db>, dim: usize, index: &IndexConfig) -> Result<Self> {
        db.use_ns(NAMESPACE).use_db(DATABASE).await?;
        let store = Self { db, dim, index: index.clone() };
        migrations::run(&store).await?;
        Ok(store)
    }

    pub fn dimension(&self) -> usize {
        self.dim
    }

    pub fn index_config(&self) -> &IndexConfig {
        &self.index
    }

    pub fn vector_index(&self) -> SurrealVectorIndex {
        SurrealVectorIndex::new(self.clone())
    }

    /// Jalankan query; galat statement mana pun dikembalikan.
    pub async fn execute(&self, sql: &str, binds: Binds) -> Result<surrealdb::IndexedResults> {
        let resp = self.db.query(sql).bind(Value::Object(binds)).await?;
        Ok(resp.check()?)
    }

    /// Baris hasil statement terakhir sebagai JSON, `id` dinormalisasi ke kunci ULID.
    pub async fn rows(&self, sql: &str, binds: Binds) -> Result<Vec<Value>> {
        let mut resp = self.execute(sql, binds).await?;
        let last = resp.num_statements().saturating_sub(1);
        let v: Value = resp.take::<surrealdb::types::Value>(last)?.into_json_value();
        Ok(match v {
            Value::Array(a) => a.into_iter().map(fix_ids).collect(),
            Value::Null => vec![],
            other => vec![fix_ids(other)],
        })
    }

    pub async fn rows_as<T: DeserializeOwned>(&self, sql: &str, binds: Binds) -> Result<Vec<T>> {
        self.rows(sql, binds)
            .await?
            .into_iter()
            .map(|r| serde_json::from_value(r).map_err(StoreError::from))
            .collect()
    }

    /// Satu nilai skalar dari statement terakhir (mis. `RETURN count(...)`).
    pub async fn scalar(&self, sql: &str, binds: Binds) -> Result<Value> {
        let mut resp = self.execute(sql, binds).await?;
        let last = resp.num_statements().saturating_sub(1);
        Ok(resp.take::<surrealdb::types::Value>(last)?.into_json_value())
    }

    /// Tulis/ganti record. `embedding` disisipkan ke dokumen bila ada.
    pub async fn upsert<T: Serialize>(&self, table: &str, id: &str, rec: &T, embedding: Option<&[f32]>) -> Result<()> {
        let content = content_of(rec, embedding)?;
        self.execute(
            "UPSERT type::record($tb, $id) CONTENT $c RETURN NONE;",
            binds! {"tb" => table, "id" => id, "c" => content},
        )
        .await?;
        Ok(())
    }

    pub async fn get<T: DeserializeOwned>(&self, table: &str, id: &str) -> Result<Option<T>> {
        let mut rows = self
            .rows("SELECT * OMIT embedding FROM type::record($tb, $id);", binds! {"tb" => table, "id" => id})
            .await?;
        rows.pop().map(serde_json::from_value).transpose().map_err(Into::into)
    }

    pub async fn require<T: DeserializeOwned>(&self, table: &str, id: &str) -> Result<T> {
        self.get(table, id).await?.ok_or_else(|| StoreError::NotFound(format!("{table}:{id}")))
    }

    /// Gabungkan sebagian field (MERGE). Nilai null menghapus field.
    pub async fn patch(&self, table: &str, id: &str, patch: Value) -> Result<()> {
        self.execute(
            "UPDATE type::record($tb, $id) MERGE $p RETURN NONE;",
            binds! {"tb" => table, "id" => id, "p" => patch},
        )
        .await?;
        Ok(())
    }

    pub async fn delete(&self, table: &str, id: &str) -> Result<()> {
        self.execute("DELETE type::record($tb, $id) RETURN NONE;", binds! {"tb" => table, "id" => id})
            .await?;
        Ok(())
    }

    pub async fn embedding_of(&self, table: &str, id: &str) -> Result<Option<Vec<f32>>> {
        let v = self
            .scalar("RETURN (SELECT VALUE embedding FROM type::record($tb, $id))[0];", binds! {"tb" => table, "id" => id})
            .await?;
        Ok(v.as_array().map(|a| a.iter().filter_map(Value::as_f64).map(|x| x as f32).collect()))
    }

    pub async fn count(&self, table: &str, where_clause: &str, binds: Binds) -> Result<u64> {
        let sql = if where_clause.is_empty() {
            format!("RETURN count(SELECT id FROM type::table('{table}'));")
        } else {
            format!("RETURN count(SELECT id FROM type::table('{table}') WHERE {where_clause});")
        };
        Ok(self.scalar(&sql, binds).await?.as_u64().unwrap_or(0))
    }

    pub fn tx(&self) -> Tx {
        Tx::new(self.clone())
    }
}

/// Dokumen yang ditulis: tanpa `id` (kunci record), tanpa field null
/// (absen = NONE), plus `embedding` bila ada.
pub(crate) fn content_of<T: Serialize>(rec: &T, embedding: Option<&[f32]>) -> Result<Value> {
    let mut v = serde_json::to_value(rec)?;
    let obj = v.as_object_mut().ok_or_else(|| StoreError::Invalid("record harus berupa objek".into()))?;
    obj.remove("id");
    obj.retain(|_, val| !val.is_null());
    if let Some(e) = embedding {
        obj.insert("embedding".into(), serde_json::json!(e));
    }
    Ok(v)
}

/// `episode:⟨01k..⟩` -> `01k..` untuk field `id`, `in`, `out`.
fn fix_ids(mut row: Value) -> Value {
    if let Some(obj) = row.as_object_mut() {
        for key in ["id", "in", "out"] {
            if let Some(Value::String(s)) = obj.get(key) {
                let fixed = record_key(s);
                obj.insert(key.into(), Value::String(fixed));
            }
        }
    }
    row
}

pub fn record_key(s: &str) -> String {
    let key = s.split_once(':').map(|(_, k)| k).unwrap_or(s);
    key.trim_matches(|c| c == '⟨' || c == '⟩' || c == '`' || c == '\'' || c == '"').to_string()
}

#[cfg(test)]
mod tests;

impl Store {
    /// Hapus & buat ulang index vektor dengan dimensi saat ini (dipakai
    /// `memory reembed` setelah mengganti model embedding).
    pub async fn rebuild_vector_indexes(&self) -> Result<()> {
        migrations::rebuild_vector_indexes(self).await
    }

    /// Buka vault yang dimensinya berbeda tanpa validasi (hanya untuk reembed).
    pub async fn open_for_reembed(dir: &Path, dim: usize, index: &IndexConfig) -> Result<Self> {
        let db = Surreal::new::<RocksDb>(dir.to_string_lossy().as_ref()).await?;
        db.use_ns(NAMESPACE).use_db(DATABASE).await?;
        Ok(Self { db, dim, index: index.clone() })
    }
}
