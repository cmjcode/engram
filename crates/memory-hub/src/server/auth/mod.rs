//! Autentikasi hub: OAuth Google/GitHub (port dari rastrum-server di atas `Store`), JWT akses,
//! refresh token berputar, tiket login untuk CLI/desktop, personal access token, dan
//! verifikasi izin repo GitHub (token dipakai sekali, tidak disimpan).

pub mod extract;
pub mod github;
pub mod handlers;
pub mod jwt;
pub mod pat;
pub mod providers;
pub mod users;
