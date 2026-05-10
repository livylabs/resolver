use axum::{extract::State,Json};

use serde::Deserialize;
use std::sync::Arc;
use crate::fetch::Fetcher;
use crate::errors::FetchError;
use serde_json::Value;
#[derive(Deserialize)]
pub struct FetchRequest{
    pub source: String 
}

pub async fn fetch_post(
    State(fetcher) : State<Arc<Fetcher>>,
    Json(payload): Json<FetchRequest>) -> Result<Json<serde_json::Value> , FetchError>{
    let data = fetcher.get_data(&payload.source, None).await?;
    Ok(Json(data))
}
#[axum::debug_handler]
pub async fn fetch_unblock(
    State(fetcher): State<Arc<Fetcher>>,
    Json(payload): Json<FetchRequest> ) -> Result<Json<Value>, FetchError> {
    let data = fetcher.unblocker(&payload.source).await?;
    Ok(Json(data))
}

        





