//! MCP tool wrapper for exact source fetching.

use crate::auth::{ResolverAuth, ResolverAuthContext, ResolverAuthError};
use crate::fetch::Fetcher;
use crate::types::FetchWithReceipt;
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header, request::Parts},
    middleware::Next,
    response::{IntoResponse, Response},
};
use rmcp::{
    ErrorData, ServerHandler,
    handler::server::tool::Extension,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, Content, Meta},
    schemars, tool, tool_handler, tool_router,
};
use serde_json::{Value, json};
use std::{fmt::Write, sync::Arc, time::Instant};

const FETCH_SOURCE_SCOPES: &[&str] = &["tool:fetch_source", "resolver:source:fetch"];
const FETCH_SOURCE_TITLE: &str = "Fetch Source";
const FETCH_SOURCE_INVOCATION_START: &str = "Fetching source";
const FETCH_SOURCE_INVOCATION_DONE: &str = "Source fetched";
const MCP_COMPAT_BODY_LIMIT: usize = 1024 * 1024;

#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct Params {
    #[schemars(
        description = "The exact source URL from the user prompt. Do not replace it with a search query or another URL."
    )]
    pub url: String,
}

pub struct Server {
    fetcher: Arc<Fetcher>,
    auth: Arc<ResolverAuth>,
}

impl Server {
    pub fn new(fetcher: Arc<Fetcher>, auth: Arc<ResolverAuth>) -> Self {
        Self { fetcher, auth }
    }
}

#[tool_router]
impl Server {
    #[tool(
        name = "fetch_source",
        description = "Fetch the exact source URL supplied by the user using the fast SmartMode proxy path. Use this whenever the prompt contains a source URL, `source: <url>`, `only take this source`, or says the URL is the source of truth. Do not perform web search or substitute another article.",
        annotations(
            title = "Fetch Source",
            read_only_hint = true,
            destructive_hint = false,
            open_world_hint = true
        ),
        meta = "fetch_source_tool_meta()"
    )]
    async fn fetch_source(
        &self,
        Extension(parts): Extension<Parts>,
        Parameters(Params { url }): Parameters<Params>,
    ) -> Result<CallToolResult, ErrorData> {
        let auth_context = match self.require_oauth(&parts).await? {
            Ok(context) => context,
            Err(result) => return Ok(result),
        };

        let started = Instant::now();
        eprintln!("mcp fetch_source start url={url}");

        let data = self
            .fetcher
            .get_fast_data_with_receipt_with_auth(&url, Some(&auth_context))
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
        let mut result = CallToolResult::structured(structured_fetch_result(&data));
        result.content = vec![Content::text(text)];
        Ok(result)
    }
}

pub async fn mirror_tools_list_security_schemes(request: Request<Body>, next: Next) -> Response {
    if request.method() != axum::http::Method::POST {
        return next.run(request).await;
    }

    let (parts, body) = request.into_parts();
    let body_bytes = match to_bytes(body, MCP_COMPAT_BODY_LIMIT).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return (
                StatusCode::PAYLOAD_TOO_LARGE,
                "MCP request body is too large for connector metadata compatibility handling",
            )
                .into_response();
        }
    };
    let should_mirror = is_tools_list_request(&body_bytes);
    let request = Request::from_parts(parts, Body::from(body_bytes));
    let response = next.run(request).await;

    if should_mirror {
        mirror_tools_list_response_security_schemes(response).await
    } else {
        response
    }
}

fn is_tools_list_request(body: &[u8]) -> bool {
    let Ok(value) = serde_json::from_slice::<Value>(body) else {
        return false;
    };

    match value {
        Value::Object(object) => object.get("method").and_then(Value::as_str) == Some("tools/list"),
        Value::Array(messages) => messages.into_iter().any(|message| {
            message
                .get("method")
                .and_then(Value::as_str)
                .is_some_and(|method| method == "tools/list")
        }),
        _ => false,
    }
}

async fn mirror_tools_list_response_security_schemes(response: Response) -> Response {
    if !response.status().is_success() {
        return response;
    }

    let (mut parts, body) = response.into_parts();
    let body_bytes = match to_bytes(body, MCP_COMPAT_BODY_LIMIT).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "MCP tools/list response is too large for connector metadata compatibility handling",
            )
                .into_response();
        }
    };

    let Ok(body_text) = std::str::from_utf8(&body_bytes) else {
        return Response::from_parts(parts, Body::from(body_bytes));
    };
    let (rewritten, changed) = enrich_tools_list_response_for_chatgpt(body_text);
    if !changed {
        return Response::from_parts(parts, Body::from(body_bytes));
    }

    parts.headers.remove(header::CONTENT_LENGTH);
    Response::from_parts(parts, Body::from(rewritten))
}

fn enrich_tools_list_response_for_chatgpt(body: &str) -> (String, bool) {
    let mut changed = false;
    let mut rewritten = String::with_capacity(body.len());

    for line in body.split_inclusive('\n') {
        let Some(rest) = line.strip_prefix("data:") else {
            rewritten.push_str(line);
            continue;
        };
        let line_ending = if rest.ends_with('\n') { "\n" } else { "" };
        let payload = rest.trim_end_matches('\n').trim_start();
        let Ok(mut value) = serde_json::from_str::<Value>(payload) else {
            rewritten.push_str(line);
            continue;
        };

        if enrich_tools_list_message_for_chatgpt(&mut value) {
            changed = true;
            rewritten.push_str("data: ");
            rewritten.push_str(&value.to_string());
            rewritten.push_str(line_ending);
        } else {
            rewritten.push_str(line);
        }
    }

    (rewritten, changed)
}

fn enrich_tools_list_message_for_chatgpt(message: &mut Value) -> bool {
    let Some(tools) = message
        .get_mut("result")
        .and_then(|result| result.get_mut("tools"))
        .and_then(Value::as_array_mut)
    else {
        return false;
    };

    let mut changed = false;
    for tool in tools {
        let Some(tool_object) = tool.as_object_mut() else {
            continue;
        };
        if tool_object.get("name").and_then(Value::as_str) == Some("fetch_source") {
            if !tool_object.contains_key("title") {
                tool_object.insert("title".to_string(), json!(FETCH_SOURCE_TITLE));
                changed = true;
            }
            if !tool_object.contains_key("outputSchema") {
                tool_object.insert("outputSchema".to_string(), fetch_source_output_schema());
                changed = true;
            }

            let meta = tool_object
                .entry("_meta".to_string())
                .or_insert_with(|| json!({}));
            if let Some(meta_object) = meta.as_object_mut() {
                let ui = meta_object
                    .entry("ui".to_string())
                    .or_insert_with(|| json!({}));
                if let Some(ui_object) = ui.as_object_mut() {
                    if !ui_object.contains_key("visibility") {
                        ui_object.insert("visibility".to_string(), chatgpt_tool_visibility());
                        changed = true;
                    }
                }
                if !meta_object.contains_key("securitySchemes") {
                    meta_object.insert(
                        "securitySchemes".to_string(),
                        fetch_source_security_schemes(),
                    );
                    changed = true;
                }
                if !meta_object.contains_key("openai/toolInvocation/invoking") {
                    meta_object.insert(
                        "openai/toolInvocation/invoking".to_string(),
                        json!(FETCH_SOURCE_INVOCATION_START),
                    );
                    changed = true;
                }
                if !meta_object.contains_key("openai/toolInvocation/invoked") {
                    meta_object.insert(
                        "openai/toolInvocation/invoked".to_string(),
                        json!(FETCH_SOURCE_INVOCATION_DONE),
                    );
                    changed = true;
                }
                if !meta_object.contains_key("openai/visibility") {
                    meta_object.insert("openai/visibility".to_string(), json!("public"));
                    changed = true;
                }
            }

            let annotations = tool_object
                .entry("annotations".to_string())
                .or_insert_with(|| json!({}));
            if let Some(annotation_object) = annotations.as_object_mut() {
                if !annotation_object.contains_key("title") {
                    annotation_object.insert("title".to_string(), json!(FETCH_SOURCE_TITLE));
                    changed = true;
                }
                if !annotation_object.contains_key("readOnlyHint") {
                    annotation_object.insert("readOnlyHint".to_string(), json!(true));
                    changed = true;
                }
                if !annotation_object.contains_key("destructiveHint") {
                    annotation_object.insert("destructiveHint".to_string(), json!(false));
                    changed = true;
                }
                if !annotation_object.contains_key("openWorldHint") {
                    annotation_object.insert("openWorldHint".to_string(), json!(true));
                    changed = true;
                }
            }
        }

        if tool_object.contains_key("securitySchemes") {
            continue;
        }

        let Some(schemes) = tool_object
            .get("_meta")
            .and_then(|meta| meta.get("securitySchemes"))
            .cloned()
        else {
            continue;
        };
        tool_object.insert("securitySchemes".to_string(), schemes);
        changed = true;
    }

    changed
}

impl Server {
    async fn require_oauth(
        &self,
        parts: &Parts,
    ) -> Result<Result<ResolverAuthContext, CallToolResult>, ErrorData> {
        match self
            .auth
            .validate_authorization_header(
                parts.headers.get(header::AUTHORIZATION),
                FETCH_SOURCE_SCOPES,
            )
            .await
        {
            Ok(context) => Ok(Ok(context)),
            Err(ResolverAuthError::ServiceUnavailable(err)) => {
                Err(ErrorData::internal_error(err, None))
            }
            Err(err) => {
                let Some((error, message)) = err.challenge_parts() else {
                    return Err(ErrorData::internal_error("OAuth validation failed", None));
                };
                Ok(Err(oauth_challenge_result(
                    &self.auth,
                    FETCH_SOURCE_SCOPES,
                    error,
                    message,
                )))
            }
        }
    }
}

fn oauth_challenge_result(
    auth: &ResolverAuth,
    required_scopes: &[&str],
    error: &'static str,
    message: &'static str,
) -> CallToolResult {
    let mut meta = Meta::new();
    meta.insert(
        "mcp/www_authenticate".to_string(),
        json!(auth.challenge(required_scopes, error, message)),
    );

    CallToolResult::error(vec![Content::text(message)]).with_meta(Some(meta))
}

fn fetch_source_tool_meta() -> Meta {
    let mut meta = Meta::new();
    meta.insert(
        "securitySchemes".to_string(),
        fetch_source_security_schemes(),
    );
    meta.insert(
        "ui".to_string(),
        json!({
            "visibility": chatgpt_tool_visibility()
        }),
    );
    meta.insert("openai/visibility".to_string(), json!("public"));
    meta.insert(
        "openai/toolInvocation/invoking".to_string(),
        json!(FETCH_SOURCE_INVOCATION_START),
    );
    meta.insert(
        "openai/toolInvocation/invoked".to_string(),
        json!(FETCH_SOURCE_INVOCATION_DONE),
    );
    meta
}

fn fetch_source_security_schemes() -> Value {
    json!([
        {
            "type": "oauth2",
            "scopes": ["tool:fetch_source"]
        }
    ])
}

fn fetch_source_output_schema() -> Value {
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "properties": {
            "receipt_id": {
                "type": "string",
                "description": "Livy resolver receipt id for this fetch."
            },
            "source_url": {
                "type": "string",
                "description": "The exact URL that was fetched."
            },
            "status": {
                "type": "integer",
                "description": "Upstream HTTP status code when available."
            },
            "fetch_elapsed_ms": {
                "type": "integer",
                "description": "Elapsed upstream fetch time in milliseconds when available."
            },
            "content_bytes": {
                "type": "integer",
                "description": "Extracted content size in bytes when available."
            },
            "text": {
                "type": "string",
                "description": "Fetched source text or serialized upstream payload."
            }
        },
        "required": ["receipt_id", "source_url", "text"],
        "additionalProperties": false
    })
}

fn chatgpt_tool_visibility() -> Value {
    json!(["model", "app"])
}

fn structured_fetch_result(data: &FetchWithReceipt) -> Value {
    let mut result = serde_json::Map::new();
    let receipt = &data.receipt;

    result.insert("receipt_id".to_string(), json!(data.receipt_id));
    result.insert("source_url".to_string(), json!(receipt.source_url));
    if let Some(status) = receipt.status {
        result.insert("status".to_string(), json!(status));
    }
    if let Some(elapsed) = receipt.duration_elapsed_ms {
        result.insert("fetch_elapsed_ms".to_string(), json!(elapsed));
    }
    if let Some(content_bytes) = receipt.content_bytes {
        result.insert("content_bytes".to_string(), json!(content_bytes));
    }
    result.insert(
        "text".to_string(),
        json!(
            extracted_content(&data.data)
                .map(str::to_string)
                .unwrap_or_else(|| data.data.to_string())
        ),
    );

    Value::Object(result)
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

#[cfg(test)]
mod tests {
    use super::{FETCH_SOURCE_SCOPES, Server, oauth_challenge_result};
    use crate::auth::ResolverAuth;
    use serde_json::json;

    #[test]
    fn fetch_source_tool_advertises_oauth_metadata() {
        let tool = Server::fetch_source_tool_attr();
        let schemes = tool
            .meta
            .as_ref()
            .and_then(|meta| meta.get("securitySchemes"))
            .and_then(|value| value.as_array())
            .expect("securitySchemes should be present");

        assert_eq!(schemes.len(), 1);
        assert_eq!(schemes[0]["type"], "oauth2");
        assert_eq!(schemes[0]["scopes"][0], "tool:fetch_source");
        assert_eq!(
            tool.meta
                .as_ref()
                .and_then(|meta| meta.get("openai/toolInvocation/invoking")),
            Some(&json!("Fetching source"))
        );
        assert_eq!(
            tool.meta.as_ref().and_then(|meta| meta.get("ui")),
            Some(&json!({ "visibility": ["model", "app"] }))
        );
        assert_eq!(
            tool.meta
                .as_ref()
                .and_then(|meta| meta.get("openai/visibility")),
            Some(&json!("public"))
        );
        assert_eq!(
            tool.meta
                .as_ref()
                .and_then(|meta| meta.get("openai/toolInvocation/invoked")),
            Some(&json!("Source fetched"))
        );
        assert_eq!(
            tool.annotations
                .as_ref()
                .and_then(|annotations| annotations.read_only_hint),
            Some(true)
        );
        assert_eq!(
            tool.annotations
                .as_ref()
                .and_then(|annotations| annotations.destructive_hint),
            Some(false)
        );
        assert_eq!(
            tool.annotations
                .as_ref()
                .and_then(|annotations| annotations.open_world_hint),
            Some(true)
        );
    }

    #[test]
    fn oauth_challenge_result_sets_mcp_www_authenticate_meta() {
        let auth = ResolverAuth::for_tests();
        let result = oauth_challenge_result(
            &auth,
            FETCH_SOURCE_SCOPES,
            "invalid_request",
            "missing bearer token",
        );
        let challenge = result
            .meta
            .as_ref()
            .and_then(|meta| meta.get("mcp/www_authenticate"))
            .and_then(|value| value.as_str())
            .expect("MCP auth challenge should be present");

        assert_eq!(result.is_error, Some(true));
        assert!(challenge.starts_with("Bearer "));
        assert!(challenge.contains(
            "resource_metadata=\"https://resolver.api.livylabs.xyz/.well-known/oauth-protected-resource\""
        ));
        assert!(challenge.contains("scope=\"tool:fetch_source\""));
        assert!(challenge.contains("error=\"invalid_request\""));
        assert!(challenge.contains("error_description=\"missing bearer token\""));
    }

    #[test]
    fn detects_tools_list_requests() {
        assert!(super::is_tools_list_request(
            br#"{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}"#
        ));
        assert!(super::is_tools_list_request(
            br#"[{"jsonrpc":"2.0","id":1,"method":"initialize"},{"jsonrpc":"2.0","id":2,"method":"tools/list"}]"#
        ));
        assert!(!super::is_tools_list_request(
            br#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{}}"#
        ));
    }

    #[test]
    fn enriches_fetch_source_descriptor_for_chatgpt() {
        let mut message = json!({
            "jsonrpc": "2.0",
            "id": 2,
            "result": {
                "tools": [{
                    "name": "fetch_source",
                    "_meta": {
                        "securitySchemes": [{
                            "type": "oauth2",
                            "scopes": ["tool:fetch_source"]
                        }]
                    }
                }]
            }
        });

        assert!(super::enrich_tools_list_message_for_chatgpt(&mut message));
        assert_eq!(message["result"]["tools"][0]["title"], "Fetch Source");
        assert_eq!(
            message["result"]["tools"][0]["outputSchema"]["required"],
            json!(["receipt_id", "source_url", "text"])
        );
        assert_eq!(
            message["result"]["tools"][0]["annotations"]["destructiveHint"],
            false
        );
        assert_eq!(
            message["result"]["tools"][0]["securitySchemes"],
            message["result"]["tools"][0]["_meta"]["securitySchemes"]
        );
        assert_eq!(
            message["result"]["tools"][0]["_meta"]["openai/toolInvocation/invoking"],
            "Fetching source"
        );
        assert_eq!(
            message["result"]["tools"][0]["_meta"]["ui"]["visibility"],
            json!(["model", "app"])
        );
        assert_eq!(
            message["result"]["tools"][0]["_meta"]["openai/visibility"],
            "public"
        );
        assert_eq!(
            message["result"]["tools"][0]["_meta"]["openai/toolInvocation/invoked"],
            "Source fetched"
        );
    }
}
