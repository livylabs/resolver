//! Axum entrypoint that mounts HTTP product routes and MCP transport.

mod api;
mod auth;
mod errors;
mod fetch;
mod mcp;
mod provenance;
mod snapshot_upload;
mod types;
use api::{
    crawl_post, extract_post, fetch_fast, fetch_post, fetch_unblock, get_receipt, map_post,
    screenshot_post, search_post, snapshot_source,
};
use axum::{
    Router,
    routing::{get, post},
};
use std::sync::Arc;
use tower::ServiceBuilder;

use crate::errors::{FetchError, Result};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};

#[tokio::main]
async fn main() -> Result<()> {
    let fetcher = Arc::new(fetch::Fetcher::new());
    let mcp_fetcher = fetcher.clone();
    let oauth = auth::AuthState::from_env()?;

    let mcp_service = StreamableHttpService::new(
        move || Ok(mcp::Server::new(mcp_fetcher.clone())),
        LocalSessionManager::default().into(),
        mcp_config(),
    );

    let mut app = Router::new()
        .route("/fetch", post(fetch_post))
        .route("/crawl", post(crawl_post))
        .route("/map", post(map_post))
        .route("/search", post(search_post))
        .route("/extract", post(extract_post))
        .route("/screenshot", post(screenshot_post))
        .route("/snapshot", post(snapshot_source))
        .route("/fetchfast", post(fetch_fast))
        .route("/fetchunblock", post(fetch_unblock))
        .route("/receipt/{id}", get(get_receipt))
        .route("/recipt/{id}", get(get_receipt))
        .route("/healthz", get(|| async { "ok" }))
        .with_state(fetcher);

    app = if let Some(oauth) = oauth {
        let mcp_service = ServiceBuilder::new()
            .layer(auth::AuthLayer::new(oauth.clone()))
            .service(mcp_service);
        app.merge(auth::router(oauth))
            .nest_service("/mcp", mcp_service)
    } else {
        app.nest_service("/mcp", mcp_service)
    };

    let port = std::env::var("PORT")
        .or_else(|_| std::env::var("RESOLVER_PORT"))
        .unwrap_or_else(|_| "3001".to_string());
    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|err| FetchError::Http(err.to_string()))?;

    axum::serve(listener, app)
        .await
        .map_err(|err| FetchError::Http(err.to_string()))?;

    Ok(())
}

fn mcp_config() -> StreamableHttpServerConfig {
    StreamableHttpServerConfig::default().disable_allowed_hosts()
}
