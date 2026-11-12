//! Pembatas laju jendela-tetap di memori (hub berjalan satu proses) untuk rute auth dan
//! percobaan join key, berkunci alamat IP klien.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Mutex;

use axum::extract::{ConnectInfo, Request, State};
use axum::http::HeaderMap;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

use super::{HubError, HubState};

#[derive(Default)]
pub struct RateLimiter {
    buckets: Mutex<HashMap<String, (u64, u32)>>,
}

impl RateLimiter {
    fn now() -> u64 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
    }

    /// Catat satu kejadian; `false` bila `key` sudah melewati `limit` dalam jendela `window_secs`.
    pub fn hit(&self, key: &str, limit: u32, window_secs: u64) -> bool {
        let w = Self::now() / window_secs.max(1);
        let mut m = self.buckets.lock().unwrap_or_else(|e| e.into_inner());
        if m.len() > 20_000 {
            m.retain(|_, (bw, _)| *bw == w);
        }
        let e = m.entry(key.to_string()).or_insert((w, 0));
        if e.0 != w {
            *e = (w, 0);
        }
        e.1 += 1;
        e.1 <= limit
    }

    /// Apakah `key` sudah melewati batas (tanpa menambah hitungan).
    pub fn blocked(&self, key: &str, limit: u32, window_secs: u64) -> bool {
        let w = Self::now() / window_secs.max(1);
        let m = self.buckets.lock().unwrap_or_else(|e| e.into_inner());
        m.get(key).is_some_and(|(bw, n)| *bw == w && *n >= limit)
    }
}

/// IP klien: hop pertama `X-Forwarded-For` bila `trust_proxy`, selain itu alamat soket.
pub fn client_ip(headers: &HeaderMap, peer: Option<SocketAddr>, trust_proxy: bool) -> String {
    if trust_proxy
        && let Some(v) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok())
        && let Some(first) = v.split(',').next().map(str::trim).filter(|s| !s.is_empty())
    {
        return first.chars().take(64).collect();
    }
    peer.map(|p| p.ip().to_string()).unwrap_or_else(|| "unknown".into())
}

/// Maks. 30 permintaan/menit per IP ke `/hub/auth/*`.
pub async fn auth_limit(State(st): State<HubState>, req: Request, next: Next) -> Response {
    let peer = req.extensions().get::<ConnectInfo<SocketAddr>>().map(|c| c.0);
    let ip = client_ip(req.headers(), peer, st.cfg.trust_proxy);
    if !st.limiter.hit(&format!("auth:{ip}"), 30, 60) {
        return HubError::TooMany("terlalu banyak permintaan login; coba lagi sebentar lagi".into()).into_response();
    }
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_counts_and_blocks() {
        let l = RateLimiter::default();
        for _ in 0..3 {
            assert!(l.hit("k", 3, 60));
        }
        assert!(!l.hit("k", 3, 60));
        assert!(l.blocked("k", 3, 60) && !l.blocked("lain", 3, 60));
        let mut h = HeaderMap::new();
        h.insert("x-forwarded-for", "10.0.0.9, 192.168.1.1".parse().unwrap());
        assert_eq!(client_ip(&h, None, true), "10.0.0.9");
        assert_eq!(client_ip(&h, Some("127.0.0.1:5".parse().unwrap()), false), "127.0.0.1");
    }
}
