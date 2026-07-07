//! Central error definitions and HTTP mapping for resolver failures.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
use livy_provenance_sdk::ProvenanceClientError;
use serde_json::json;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, FetchError>;

#[derive(Error, Debug)]
pub enum FetchError {
    #[error("Can't fetch page (Clean)")]
    UnableFetch(#[from] reqwest::Error),
    #[error("Output is not desearializable")]
    UnableToSerialize(#[from] serde_json::Error),
    #[error("Http Failed")]
    Http(String),
    #[error("Upstream fetch failed")]
    Upstream(String),
    #[error("Fetch timed out")]
    Timeout(String),
    #[error("Not found")]
    NotFound(String),
    #[error("Bad request")]
    BadRequest(String),
    #[error("Payment required")]
    PaymentRequired(String),
    #[error("Credit authorization failed")]
    Credits(String),
    #[error("Snapshot failed")]
    Snapshot(#[from] SnapshotError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolverAuthError {
    Unauthorized {
        error: &'static str,
        message: &'static str,
    },
    Forbidden {
        error: &'static str,
        message: &'static str,
    },
    ServiceUnavailable(String),
}

impl ResolverAuthError {
    pub fn challenge_parts(&self) -> Option<(&'static str, &'static str)> {
        match self {
            ResolverAuthError::Unauthorized { error, message }
            | ResolverAuthError::Forbidden { error, message } => Some((error, message)),
            ResolverAuthError::ServiceUnavailable(_) => None,
        }
    }
}

#[derive(Debug, Error)]
pub enum ResolverCreditsError {
    #[error("OAuth context is missing Livy {0}")]
    MissingAuth(&'static str),
    #[error("credit request header is invalid: {0}")]
    InvalidHeader(String),
    #[error("credit request failed: {0}")]
    Http(reqwest::Error),
    #[error("credit request returned {status}: {}", compact_body(body))]
    Backend { status: StatusCode, body: String },
}

impl ResolverCreditsError {
    pub fn is_payment_required(&self) -> bool {
        match self {
            Self::Backend { status, body } => {
                *status == StatusCode::PAYMENT_REQUIRED
                    || backend_error_code(body).as_deref() == Some("insufficient_user_credits")
            }
            _ => false,
        }
    }
}

#[derive(Debug, Error)]
pub enum ProvenanceError {
    #[error("{0} must be set when provenance is enabled")]
    MissingEnv(&'static str),
    #[error("invalid provenance configuration: {0}")]
    InvalidEnv(String),
    #[error("provenance SDK failed: {0}")]
    Sdk(#[from] ProvenanceClientError),
    #[error("provenance HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("provenance backend returned {status}: {body}")]
    Backend { status: StatusCode, body: String },
    #[error("provenance JSON handling failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("provenance attestation failed: {0}")]
    Attestation(String),
    #[error("provenance timestamp failed: {0}")]
    Time(String),
}

#[derive(Debug, Error)]
pub enum SnapshotError {
    #[error("Spider snapshot response did not include raw HTML")]
    MissingHtml,
    #[error("Spider snapshot response did not include a screenshot")]
    MissingScreenshot,
}

impl IntoResponse for FetchError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            FetchError::UnableFetch(e) => (StatusCode::CREATED, format!("Fetch failed {} ", e)),
            FetchError::UnableToSerialize(e) => (
                StatusCode::BAD_REQUEST,
                format!("Data is not serializable {}", e),
            ),
            FetchError::Http(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Http failed {}", e),
            ),
            FetchError::Upstream(e) => (
                StatusCode::BAD_GATEWAY,
                format!("Upstream fetch failed {}", e),
            ),
            FetchError::Timeout(e) => (
                StatusCode::GATEWAY_TIMEOUT,
                format!("Fetch timed out {}", e),
            ),
            FetchError::NotFound(e) => (StatusCode::NOT_FOUND, e),
            FetchError::BadRequest(e) => (StatusCode::BAD_REQUEST, e),
            FetchError::PaymentRequired(e) => (
                StatusCode::PAYMENT_REQUIRED,
                format!("Payment required {}", e),
            ),
            FetchError::Credits(e) => (
                StatusCode::BAD_GATEWAY,
                format!("Credit authorization failed {}", e),
            ),
            FetchError::Snapshot(e) => (StatusCode::BAD_REQUEST, format!("Snapshot failed {}", e)),
        };

        let body = match status {
            StatusCode::PAYMENT_REQUIRED => Json(json!({
                "code": "payment_required",
                "error": message
            })),
            _ => Json(json!({
                "error" : message
            })),
        };
        (status, body).into_response()
    }
}

fn backend_error_code(body: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("code")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        })
}

fn compact_body(body: &str) -> String {
    let body = body.trim();
    if body.len() <= 512 {
        return body.to_string();
    }
    format!("{}...", &body[..512])
}
