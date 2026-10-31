//! Pencarian vektor. `VectorIndex` adalah titik ganti bila kelak vektor
//! dipindah ke mesin terpisah (Qdrant, LanceDB) untuk skala sangat besar.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::{Result, Store, binds};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VectorHit {
    pub id: String,
    /// Kemiripan kosinus (1 = identik).
    pub similarity: f32,
}

#[async_trait]
pub trait VectorIndex: Send + Sync {
    /// Cari `k` tetangga terdekat di `table` milik `subject`. `filter` adalah
    /// klausa tambahan SurrealQL yang aman (konstanta, bukan input pengguna).
    async fn search(&self, table: &str, subject: &str, vector: &[f32], k: usize, filter: &str) -> Result<Vec<VectorHit>>;
}

pub struct SurrealVectorIndex {
    store: Store,
}

impl SurrealVectorIndex {
    pub fn new(store: Store) -> Self {
        Self { store }
    }
}

#[async_trait]
impl VectorIndex for SurrealVectorIndex {
    async fn search(&self, table: &str, subject: &str, vector: &[f32], k: usize, filter: &str) -> Result<Vec<VectorHit>> {
        if vector.len() != self.store.dimension() || k == 0 {
            return Ok(vec![]);
        }
        let ef = self.store.index_config().ef_search.max(k as u32);
        // Ambil lebih banyak kandidat karena filter subjek/status diterapkan
        // setelah traversal index.
        let k_index = (k * 4).max(16);
        let extra = if filter.is_empty() { String::new() } else { format!(" AND ({filter})") };
        let sql = format!(
            "SELECT id, vector::distance::knn() AS dist FROM type::table($tb) \
             WHERE embedding <|{k_index},{ef}|> $v AND subject = $s{extra} ORDER BY dist LIMIT {k};"
        );
        let rows = self.store.rows(&sql, binds! {"tb" => table, "v" => vector, "s" => subject}).await?;
        Ok(rows
            .into_iter()
            .filter_map(|r| {
                let id = r["id"].as_str()?.to_string();
                let dist = r["dist"].as_f64()? as f32;
                Some(VectorHit { id, similarity: 1.0 - dist })
            })
            .collect())
    }
}
