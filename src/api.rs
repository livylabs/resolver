//! HTTP handlers that translate product routes into fetcher calls.

use axum::{
    Json,
    extract::{Extension, Path, State},
};

use crate::auth::ResolverAuthContext;
use crate::credits::ResolverCreditsClient;
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
    Extension(auth_context): Extension<ResolverAuthContext>,
    Extension(credits): Extension<Arc<ResolverCreditsClient>>,
    Json(payload): Json<ProductRequest>,
) -> Result<Json<ProductResponse>, FetchError> {
    debit_product_route(
        &credits,
        &auth_context,
        ProductRoute::Scrape.as_str(),
        payload.source.as_deref(),
        None,
    )
    .await?;
    let data = fetcher.product_fetch(payload, ProductRoute::Scrape).await?;
    Ok(Json(data))
}

pub async fn crawl_post(
    State(fetcher): State<Arc<Fetcher>>,
    Extension(auth_context): Extension<ResolverAuthContext>,
    Extension(credits): Extension<Arc<ResolverCreditsClient>>,
    Json(payload): Json<ProductRequest>,
) -> Result<Json<ProductResponse>, FetchError> {
    debit_product_route(
        &credits,
        &auth_context,
        ProductRoute::Crawl.as_str(),
        payload.source.as_deref(),
        None,
    )
    .await?;
    let data = fetcher.product_fetch(payload, ProductRoute::Crawl).await?;
    Ok(Json(data))
}

pub async fn map_post(
    State(fetcher): State<Arc<Fetcher>>,
    Extension(auth_context): Extension<ResolverAuthContext>,
    Extension(credits): Extension<Arc<ResolverCreditsClient>>,
    Json(payload): Json<ProductRequest>,
) -> Result<Json<ProductResponse>, FetchError> {
    debit_product_route(
        &credits,
        &auth_context,
        ProductRoute::Map.as_str(),
        payload.source.as_deref(),
        None,
    )
    .await?;
    let data = fetcher.product_fetch(payload, ProductRoute::Map).await?;
    Ok(Json(data))
}

pub async fn search_post(
    State(fetcher): State<Arc<Fetcher>>,
    Extension(auth_context): Extension<ResolverAuthContext>,
    Extension(credits): Extension<Arc<ResolverCreditsClient>>,
    Json(payload): Json<ProductRequest>,
) -> Result<Json<ProductResponse>, FetchError> {
    debit_product_route(
        &credits,
        &auth_context,
        ProductRoute::Search.as_str(),
        None,
        None,
    )
    .await?;
    let data = fetcher.product_fetch(payload, ProductRoute::Search).await?;
    Ok(Json(data))
}

pub async fn extract_post(
    State(fetcher): State<Arc<Fetcher>>,
    Extension(auth_context): Extension<ResolverAuthContext>,
    Extension(credits): Extension<Arc<ResolverCreditsClient>>,
    Json(payload): Json<ProductRequest>,
) -> Result<Json<ProductResponse>, FetchError> {
    debit_product_route(
        &credits,
        &auth_context,
        ProductRoute::Extract.as_str(),
        payload.source.as_deref(),
        None,
    )
    .await?;
    let data = fetcher
        .product_fetch(payload, ProductRoute::Extract)
        .await?;
    Ok(Json(data))
}

pub async fn screenshot_post(
    State(fetcher): State<Arc<Fetcher>>,
    Extension(auth_context): Extension<ResolverAuthContext>,
    Extension(credits): Extension<Arc<ResolverCreditsClient>>,
    Json(payload): Json<ProductRequest>,
) -> Result<Json<ProductResponse>, FetchError> {
    debit_product_route(
        &credits,
        &auth_context,
        ProductRoute::Screenshot.as_str(),
        payload.source.as_deref(),
        None,
    )
    .await?;
    let data = fetcher
        .product_fetch(payload, ProductRoute::Screenshot)
        .await?;
    Ok(Json(data))
}

pub async fn fetch_fast(
    State(fetcher): State<Arc<Fetcher>>,
    Extension(auth_context): Extension<ResolverAuthContext>,
    Extension(credits): Extension<Arc<ResolverCreditsClient>>,
    Json(payload): Json<FetchRequest>,
) -> Result<Json<FetchWithReceipt>, FetchError> {
    debit_product_route(
        &credits,
        &auth_context,
        "fetchfast",
        Some(&payload.source),
        None,
    )
    .await?;
    let data = fetcher.get_fast_data_with_receipt(&payload.source).await?;
    Ok(Json(data))
}

pub async fn get_receipt(
    State(fetcher): State<Arc<Fetcher>>,
    Extension(auth_context): Extension<ResolverAuthContext>,
    Extension(credits): Extension<Arc<ResolverCreditsClient>>,
    Path(id): Path<String>,
) -> Result<Json<Receipt>, FetchError> {
    debit_product_route(&credits, &auth_context, "receipt", None, Some(&id)).await?;
    fetcher
        .get_receipt(&id)
        .map(Json)
        .ok_or_else(|| FetchError::NotFound(format!("Receipt not found: {id}")))
}

pub async fn snapshot_source(
    State(fetcher): State<Arc<Fetcher>>,
    Extension(auth_context): Extension<ResolverAuthContext>,
    Extension(credits): Extension<Arc<ResolverCreditsClient>>,
    Json(payload): Json<FetchRequest>,
) -> Result<Json<crate::snapshot_upload::SnapshotPayload>, FetchError> {
    debit_product_route(
        &credits,
        &auth_context,
        "snapshot",
        Some(&payload.source),
        None,
    )
    .await?;
    let snapshot = fetcher.snapshot_with_receipt(&payload.source).await?;
    Ok(Json(snapshot))
}

#[axum::debug_handler]
pub async fn fetch_unblock(
    State(fetcher): State<Arc<Fetcher>>,
    Extension(auth_context): Extension<ResolverAuthContext>,
    Extension(credits): Extension<Arc<ResolverCreditsClient>>,
    Json(payload): Json<FetchRequest>,
) -> Result<Json<Value>, FetchError> {
    debit_product_route(
        &credits,
        &auth_context,
        "fetchunblock",
        Some(&payload.source),
        None,
    )
    .await?;
    let data = fetcher.unblocker(&payload.source).await?;
    Ok(Json(data))
}

async fn debit_product_route(
    credits: &ResolverCreditsClient,
    auth_context: &ResolverAuthContext,
    route: &str,
    source_url: Option<&str>,
    subject_id: Option<&str>,
) -> Result<(), FetchError> {
    match credits
        .debit_product_request(auth_context, route, source_url, subject_id)
        .await
    {
        Ok(Some(outcome)) => {
            eprintln!(
                "resolver product credits route={} charged={} amount={} mode={} enforced={}",
                route, outcome.charged, outcome.amount, outcome.mode, outcome.enforced
            );
            Ok(())
        }
        Ok(None) => {
            eprintln!("resolver product credits route={route} skipped");
            Ok(())
        }
        Err(err) if err.is_payment_required() => Err(FetchError::PaymentRequired(err.to_string())),
        Err(err) => Err(FetchError::Credits(err.to_string())),
    }
}
