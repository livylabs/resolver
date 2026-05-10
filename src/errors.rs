use thiserror::Error;
use std::io;
use axum::response::{IntoResponse, Response, Json};
use serde_json::json;
use reqwest::StatusCode;

#[derive(Error,Debug)]
pub enum FetchError {
    #[error("Can't fetch page (Clean)")]
    UnableFetch(#[from] reqwest::Error),
    #[error("Output is not desearializable")]
    UnableToSerialize(#[from] serde_json::Error)
}

impl IntoResponse for FetchError {
    fn into_response(self) -> Response {
        let (status , message) = match self {
            FetchError::UnableFetch(e) => (
                StatusCode::CREATED, format!("Fetch failed {} ", e),
            ), 
            FetchError::UnableToSerialize(e) => (
                StatusCode::BAD_REQUEST , format!("Data is not serializable {}" ,e)
            )
        };

        let body = Json(json!({
            "error" : message
        }));
       (status, body).into_response() 
    } 

}




