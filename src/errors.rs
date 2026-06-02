//! HTTP error mapping for product and upstream failures.

use crate::snapshot_upload::SnapshotError;
use axum::{
    http::StatusCode,
    response::{IntoResponse, Json, Response},
};
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
    #[error("Snapshot failed")]
    Snapshot(#[from] SnapshotError),
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
            FetchError::Snapshot(e) => (StatusCode::BAD_REQUEST, format!("Snapshot failed {}", e)),
        };

        let body = Json(json!({
            "error" : message
        }));
        (status, body).into_response()
    }
}
