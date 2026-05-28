//! MCP tool wrapper for exact source fetching.

use crate::fetch::Fetcher;
use rmcp::{
    ErrorData, ServerHandler,
    handler::server::wrapper::{Json, Parameters},
    schemars, tool, tool_handler, tool_router,
};

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct Params {
    #[schemars(
        description = "The exact source URL from the user prompt. Do not replace it with a search query or another URL."
    )]
    pub url: String,
}

pub struct Server {
    fetcher: std::sync::Arc<Fetcher>,
}

impl Server {
    pub fn new(fetcher: std::sync::Arc<Fetcher>) -> Self {
        Self { fetcher }
    }
}

#[tool_router]
impl Server {
    #[tool(
        name = "fetch_source",
        description = "Fetch the exact source URL supplied by the user using the fast SmartMode proxy path. Use this whenever the prompt contains a source URL, `source: <url>`, `only take this source`, or says the URL is the source of truth. Do not perform web search or substitute another article."
    )]
    async fn fetch_source(
        &self,
        Parameters(Params { url }): Parameters<Params>,
    ) -> Result<Json<serde_json::Value>, ErrorData> {
        let data = self
            .fetcher
            .get_fast_data_with_receipt(&url)
            .await
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        serde_json::to_value(data)
            .map(Json)
            .map_err(|e| ErrorData::internal_error(e.to_string(), None))
    }
}

#[tool_handler(
    name = "livygensyn-source-fetcher",
    version = "0.1.0",
    instructions = "This server only fetches user-provided source URLs through the fast SmartMode proxy path. If a user prompt includes a URL or phrases like `source:`, `only take this source`, or `source of truth`, call `fetch_source` with that exact URL before answering. Do not search the web, do not use rumors or alternate sources, and do not replace the URL with a query."
)]
impl ServerHandler for Server {}
