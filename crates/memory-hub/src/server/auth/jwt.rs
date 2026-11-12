//! Token akses hub: JWT HS256 berumur pendek (`[hub] access_minutes`), berisi id, email, dan
//! nama agar handler tidak perlu membaca tabel pengguna pada tiap permintaan.

use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};

use super::super::models::UserRow;
use super::super::{HubError, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub email: String,
    #[serde(default)]
    pub name: String,
    pub iat: usize,
    pub exp: usize,
    #[serde(default)]
    pub typ: String,
}

pub fn issue(user: &UserRow, secret: &str, minutes: i64) -> Result<String> {
    let now = chrono::Utc::now();
    let claims = Claims {
        sub: user.id.clone(),
        email: user.email.clone(),
        name: user.name.clone(),
        iat: now.timestamp() as usize,
        exp: (now + chrono::Duration::minutes(minutes.max(1))).timestamp() as usize,
        typ: "access".into(),
    };
    encode(&Header::default(), &claims, &EncodingKey::from_secret(secret.as_bytes())).map_err(|e| HubError::Internal(format!("jwt: {e}")))
}

pub fn verify(token: &str, secret: &str) -> Result<Claims> {
    let data = decode::<Claims>(token, &DecodingKey::from_secret(secret.as_bytes()), &Validation::default()).map_err(|e| HubError::Unauthorized(format!("token akses tidak valid: {e}")))?;
    if data.claims.typ != "access" {
        return Err(HubError::Unauthorized("token bukan token akses".into()));
    }
    Ok(data.claims)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_wrong_secret() {
        let u = UserRow { id: "u1".into(), email: "a@b.c".into(), name: "A".into(), ..Default::default() };
        let t = issue(&u, "rahasia-yang-cukup-panjang-untuk-uji-123", 5).unwrap();
        let c = verify(&t, "rahasia-yang-cukup-panjang-untuk-uji-123").unwrap();
        assert_eq!((c.sub.as_str(), c.email.as_str(), c.name.as_str()), ("u1", "a@b.c", "A"));
        assert!(verify(&t, "rahasia-lain-yang-juga-panjang-sekali-456").is_err());
        assert!(verify("bukan.jwt.sama.sekali", "x").is_err());
    }
}
