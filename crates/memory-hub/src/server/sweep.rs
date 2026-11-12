//! Pembersih berkala: state OAuth, tiket, dan sesi kedaluwarsa; record yang sudah dicabut
//! lebih lama dari `record_retention_days`.

use std::time::Duration;

use memory_core::ids::now_rfc3339;
use memory_store::binds;

use super::{HubState, Result};

pub async fn sweep(st: &HubState) -> Result<()> {
    let now = now_rfc3339();
    st.store.execute("DELETE hub_oauth_state WHERE expires_at < $now;", binds! {"now" => now}).await?;
    st.store.execute("DELETE hub_ticket WHERE expires_at < $now;", binds! {"now" => now}).await?;
    st.store.execute("DELETE hub_session WHERE expires_at < $now;", binds! {"now" => now}).await?;
    let days = st.cfg.record_retention_days;
    if days > 0 {
        let cutoff = (chrono::Utc::now() - chrono::Duration::days(days as i64)).format("%Y-%m-%dT%H:%M:%SZ").to_string();
        st.store.execute("DELETE hub_record WHERE status = 'withdrawn' AND withdrawn_at < $cut;", binds! {"cut" => cutoff}).await?;
    }
    Ok(())
}

pub fn spawn(st: HubState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(600)).await;
            if let Err(e) = sweep(&st).await {
                tracing::warn!(error = %e, "pembersihan hub gagal");
            }
        }
    })
}
