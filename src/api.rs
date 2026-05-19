//! HTTP handlers that translate product routes into fetcher calls.

use axum::{
    Json,
    extract::{Path, State},
};

use crate::errors::FetchError;
use crate::fetch::Fetcher;
use crate::types::{FetchWithReceipt, ProductRequest, ProductResponse, ProductRoute, Receipt};
use serde::Deserialize;
use serde_json::Value;
use std::sync::Arc;

#[derive(Deserialize)]
pub struct FetchRequest {
    pub source: String,
}

pub async fn fetch_post(
    State(fetcher): State<Arc<Fetcher>>,
    Json(payload): Json<ProductRequest>,
) -> Result<Json<ProductResponse>, FetchError> {
    let data = fetcher.product_fetch(payload, ProductRoute::Scrape).await?;
    Ok(Json(data))
}

pub async fn crawl_post(
    State(fetcher): State<Arc<Fetcher>>,
    Json(payload): Json<ProductRequest>,
) -> Result<Json<ProductResponse>, FetchError> {
    let data = fetcher.product_fetch(payload, ProductRoute::Crawl).await?;
    Ok(Json(data))
}

pub async fn map_post(
    State(fetcher): State<Arc<Fetcher>>,
    Json(payload): Json<ProductRequest>,
) -> Result<Json<ProductResponse>, FetchError> {
    let data = fetcher.product_fetch(payload, ProductRoute::Map).await?;
    Ok(Json(data))
}

pub async fn search_post(
    State(fetcher): State<Arc<Fetcher>>,
    Json(payload): Json<ProductRequest>,
) -> Result<Json<ProductResponse>, FetchError> {
    let data = fetcher.product_fetch(payload, ProductRoute::Search).await?;
    Ok(Json(data))
}

pub async fn extract_post(
    State(fetcher): State<Arc<Fetcher>>,
    Json(payload): Json<ProductRequest>,
) -> Result<Json<ProductResponse>, FetchError> {
    let data = fetcher
        .product_fetch(payload, ProductRoute::Extract)
        .await?;
    Ok(Json(data))
}

pub async fn screenshot_post(
    State(fetcher): State<Arc<Fetcher>>,
    Json(payload): Json<ProductRequest>,
) -> Result<Json<ProductResponse>, FetchError> {
    let data = fetcher
        .product_fetch(payload, ProductRoute::Screenshot)
        .await?;
    Ok(Json(data))
}

pub async fn fetch_fast(
    State(fetcher): State<Arc<Fetcher>>,
    Json(payload): Json<FetchRequest>,
) -> Result<Json<FetchWithReceipt>, FetchError> {
    let data = fetcher.get_fast_data_with_receipt(&payload.source).await?;
    Ok(Json(data))
}

pub async fn get_receipt(
    State(fetcher): State<Arc<Fetcher>>,
    Path(id): Path<String>,
) -> Result<Json<Receipt>, FetchError> {
    fetcher
        .get_receipt(&id)
        .map(Json)
        .ok_or_else(|| FetchError::NotFound(format!("Receipt not found: {id}")))
}

#[axum::debug_handler]
pub async fn fetch_unblock(
    State(fetcher): State<Arc<Fetcher>>,
    Json(payload): Json<FetchRequest>,
) -> Result<Json<Value>, FetchError> {
    let data = fetcher.unblocker(&payload.source).await?;
    Ok(Json(data))
}
