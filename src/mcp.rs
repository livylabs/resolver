//! MCP tool wrapper for exact source fetching.

use crate::fetch::Fetcher;
use crate::types::FetchWithReceipt;
use rmcp::{
    ErrorData, ServerHandler,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, Content},
    schemars, tool, tool_handler, tool_router,
};
use serde_json::Value;
use std::{fmt::Write, time::Instant};

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
    ) -> Result<CallToolResult, ErrorData> {
        let started = Instant::now();
        eprintln!("mcp fetch_source start url={url}");

        let data = self
            .fetcher
            .get_fast_data_with_receipt(&url)
            .await
            .map_err(|e| {
                eprintln!(
                    "mcp fetch_source error elapsed_ms={} error={e}",
                    started.elapsed().as_millis()
                );
                ErrorData::internal_error(e.to_string(), None)
            })?;

        let text = render_fetch_result(&data);
        eprintln!(
            "mcp fetch_source ok elapsed_ms={} response_bytes={}",
            started.elapsed().as_millis(),
            text.len()
        );
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }
}

fn render_fetch_result(data: &FetchWithReceipt) -> String {
    let mut output = String::new();
    let receipt = &data.receipt;

    let _ = writeln!(output, "receipt_id: {}", data.receipt_id);
    let _ = writeln!(output, "status: {}", display_option(receipt.status));
    let _ = writeln!(
        output,
        "fetch_elapsed_ms: {}",
        display_option(receipt.duration_elapsed_ms)
    );
    let _ = writeln!(
        output,
        "content_bytes: {}",
        display_option(receipt.content_bytes)
    );
    output.push_str("\n---\n\n");
    match extracted_content(&data.data) {
        Some(content) => output.push_str(content),
        None => output.push_str(&data.data.to_string()),
    }

    output
}

fn display_option<T: std::fmt::Display>(value: Option<T>) -> String {
    value
        .map(|value| value.to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn extracted_content(data: &Value) -> Option<&str> {
    if let Some(content) = data.get("content").and_then(Value::as_str) {
        return Some(content);
    }

    data.as_array()?
        .iter()
        .find_map(|item| item.get("content").and_then(Value::as_str))
}

#[tool_handler(
    name = "livygensyn-source-fetcher",
    version = "0.1.0",
    instructions = "This server only fetches user-provided source URLs through the fast SmartMode proxy path. If a user prompt includes a URL or phrases like `source:`, `only take this source`, or `source of truth`, call `fetch_source` with that exact URL before answering. Do not search the web, do not use rumors or alternate sources, and do not replace the URL with a query."
)]
impl ServerHandler for Server {}
