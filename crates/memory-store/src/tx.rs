//! Transaksi: kumpulkan banyak statement lalu jalankan sekaligus di dalam
//! `BEGIN … COMMIT`, sehingga satu episode beserta fakta & entitasnya
//! tersimpan utuh atau tidak sama sekali.

use serde::Serialize;
use serde_json::Value;

use crate::{Binds, Result, Store, content_of};

pub struct Tx {
    store: Store,
    stmts: Vec<String>,
    binds: Binds,
    n: usize,
}

impl Tx {
    pub(crate) fn new(store: Store) -> Self {
        Self { store, stmts: vec![], binds: Binds::new(), n: 0 }
    }

    fn var(&mut self, v: Value) -> String {
        self.n += 1;
        let name = format!("p{}", self.n);
        self.binds.insert(name.clone(), v);
        format!("${name}")
    }

    pub fn upsert<T: Serialize>(&mut self, table: &str, id: &str, rec: &T, embedding: Option<&[f32]>) -> Result<&mut Self> {
        let c = content_of(rec, embedding)?;
        let tb = self.var(Value::String(table.into()));
        let idv = self.var(Value::String(id.into()));
        let cv = self.var(c);
        self.stmts.push(format!("UPSERT type::record({tb}, {idv}) CONTENT {cv} RETURN NONE;"));
        Ok(self)
    }

    pub fn patch(&mut self, table: &str, id: &str, patch: Value) -> &mut Self {
        let tb = self.var(Value::String(table.into()));
        let idv = self.var(Value::String(id.into()));
        let pv = self.var(patch);
        self.stmts.push(format!("UPDATE type::record({tb}, {idv}) MERGE {pv} RETURN NONE;"));
        self
    }

    /// Statement mentah dengan variabel sendiri (nama variabel harus unik dalam tx).
    pub fn raw(&mut self, sql: &str, binds: Binds) -> &mut Self {
        for (k, v) in binds {
            self.binds.insert(k, v);
        }
        self.stmts.push(sql.to_string());
        self
    }

    pub fn len(&self) -> usize {
        self.stmts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.stmts.is_empty()
    }

    pub async fn commit(self) -> Result<()> {
        if self.stmts.is_empty() {
            return Ok(());
        }
        let sql = format!("BEGIN TRANSACTION;\n{}\nCOMMIT TRANSACTION;", self.stmts.join("\n"));
        self.store.execute(&sql, self.binds).await?;
        Ok(())
    }
}
