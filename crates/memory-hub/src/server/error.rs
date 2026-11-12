//! Galat hub -> status HTTP + `{"error": "..."}` (bentuk yang sama dengan API server lokal).

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

#[derive(Debug, thiserror::Error)]
pub enum HubError {
    #[error("{0}")]
    BadRequest(String),
    #[error("{0}")]
    Unauthorized(String),
    #[error("{0}")]
    Forbidden(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    TooMany(String),
    /// Provider OAuth/GitHub tidak bisa dihubungi atau menjawab aneh.
    #[error("{0}")]
    OAuth(String),
    #[error("{0}")]
    Internal(String),
}

impl From<memory_store::StoreError> for HubError {
    fn from(e: memory_store::StoreError) -> Self {
        HubError::Internal(e.to_string())
    }
}

impl From<serde_json::Error> for HubError {
    fn from(e: serde_json::Error) -> Self {
        HubError::Internal(format!("serde: {e}"))
    }
}

impl HubError {
    pub fn status(&self) -> StatusCode {
        match self {
            HubError::BadRequest(_) => StatusCode::BAD_REQUEST,
            HubError::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            HubError::Forbidden(_) => StatusCode::FORBIDDEN,
            HubError::NotFound(_) => StatusCode::NOT_FOUND,
            HubError::TooMany(_) => StatusCode::TOO_MANY_REQUESTS,
            HubError::OAuth(_) => StatusCode::BAD_GATEWAY,
            HubError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl IntoResponse for HubError {
    fn into_response(self) -> Response {
        let status = self.status();
        if status.is_server_error() {
            tracing::error!(error = %self, "galat hub");
        }
        (status, axum::Json(json!({"error": self.to_string()}))).into_response()
    }
}

pub type Result<T> = std::result::Result<T, HubError>;
pub type Api<T> = Result<axum::Json<T>>;
