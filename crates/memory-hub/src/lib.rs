//! Hub tim engram.
//!
//! Vault tiap anggota tetap menjadi penyimpanan & mesin query; hub hanya (1) otoritas akses
//! (akun Google/GitHub, keanggotaan repo, join key) dan (2) jalur kirim-terima memori proyek
//! terdistilasi antar anggota lewat change log per repo (`seq` monoton, pull berbasis cursor).
//! Tidak ada teks sesi mentah maupun embedding yang pernah disimpan di hub.
//!
//! - [`types`]: tipe wire yang dipakai server dan klien.
//! - [`client`]: kredensial (`<data_dir>/hub.json`), alur login lewat tiket, klien HTTP.
//! - [`server`] (fitur `server`): router axum `/hub/*`, tabel `hub_*` di `Store` yang sama.

pub mod client;
pub mod ids;
pub mod types;

#[cfg(feature = "server")]
pub mod server;

#[cfg(feature = "server")]
pub use server::{HubSecrets, HubState, router};
