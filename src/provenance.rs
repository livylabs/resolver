//! Livy provenance client for resolver fetch attestations.

use crate::types::{
    FormatSelection, ProductFormat, ProductMode, ProductProxy, ProductRequest, ProductRoute,
    Receipt,
};
use livy_tee::Livy;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fmt,
    sync::atomic::{AtomicBool, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

const DEFAULT_SCHEMA_ID: &str = "resolver-fetch-v1";
const DEFAULT_SCHEMA_VERSION: &str = "1";
const DEFAULT_INTEGRATION_ID: &str = "delphi";
const APPLICATION_DOMAIN: &str = "resolver";
const SUBJECT_TYPE: &str = "resolver_fetch";
const ATTESTATION_CLAIM: &str = "source";

#[derive(Debug)]
pub struct ProvenanceClient {
    config: ProvenanceConfig,
    http: reqwest::Client,
    livy: Livy,
    template_ready: AtomicBool,
}

#[derive(Debug, Clone)]
struct ProvenanceConfig {
    backend_base_url: String,
    explorer_base_url: Option<String>,
    api_key: String,
    integration_id: String,
    schema_id: String,
    schema_version: String,
    visibility: String,
    verification_mode: String,
    subject_prefix: String,
    program_id: Option<String>,
    bootstrap_template: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ProvenanceResult {
    pub provenance_attestation_id: String,
    pub subject_id: String,
    pub schema_id: String,
    pub schema_version: String,
    pub verification_status: String,
    pub schema_binding_status: String,
    pub public_values_commitment: String,
    pub report_payload_hash: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explorer_url: Option<String>,
    pub data_sha256: String,
}

#[derive(Debug)]
pub enum ProvenanceError {
    MissingEnv(&'static str),
    InvalidEnv(String),
    Http(reqwest::Error),
    Json(serde_json::Error),
    Attestation(String),
    Backend(String),
    Time(String),
}

impl fmt::Display for ProvenanceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingEnv(name) => write!(f, "{name} must be set when provenance is enabled"),
            Self::InvalidEnv(message) => write!(f, "invalid provenance configuration: {message}"),
            Self::Http(err) => write!(f, "provenance HTTP request failed: {err}"),
            Self::Json(err) => write!(f, "provenance JSON handling failed: {err}"),
            Self::Attestation(err) => write!(f, "provenance attestation failed: {err}"),
            Self::Backend(err) => write!(f, "provenance backend rejected the record: {err}"),
            Self::Time(err) => write!(f, "provenance timestamp failed: {err}"),
        }
    }
}

impl std::error::Error for ProvenanceError {}

impl From<reqwest::Error> for ProvenanceError {
    fn from(err: reqwest::Error) -> Self {
        Self::Http(err)
    }
}

impl From<serde_json::Error> for ProvenanceError {
    fn from(err: serde_json::Error) -> Self {
        Self::Json(err)
    }
}

#[derive(Debug)]
pub struct ResolverFetchEvidence<'a> {
    pub payload: &'a ProductRequest,
    pub route: ProductRoute,
    pub mode: ProductMode,
    pub data: &'a Value,
    pub receipt: Option<&'a Receipt>,
}

#[derive(Debug, Deserialize)]
struct ProvenanceRecordResponse {
    provenance_attestation_id: String,
    subject_id: String,
    verification_status: String,
    schema_binding_status: String,
    public_values_commitment: String,
    report_payload_hash: String,
}

impl ProvenanceClient {
    pub fn from_env() -> Result<Option<Self>, ProvenanceError> {
        let configured = env_present("LIVY_BACKEND_BASE_URL")
            || env_present("LIVY_API_KEY")
            || env_present("LIVY_PROVENANCE_ENABLED");
        let enabled = env_bool("LIVY_PROVENANCE_ENABLED")?.unwrap_or(configured);

        if !enabled {
            return Ok(None);
        }

        let backend_base_url = required_env("LIVY_BACKEND_BASE_URL")?;
        let api_key = required_env("LIVY_API_KEY")?;
        let integration_id = env_or("LIVY_INTEGRATION_ID", DEFAULT_INTEGRATION_ID);
        let schema_id = env_or("LIVY_PROVENANCE_SCHEMA_ID", DEFAULT_SCHEMA_ID);
        let schema_version = env_or("LIVY_PROVENANCE_SCHEMA_VERSION", DEFAULT_SCHEMA_VERSION);
        let visibility = env_or("LIVY_PROVENANCE_VISIBILITY", "public");
        let verification_mode = env_or("LIVY_PROVENANCE_VERIFICATION_MODE", "verify_fresh");
        let subject_prefix = env_or("LIVY_PROVENANCE_SUBJECT_PREFIX", SUBJECT_TYPE);
        let explorer_base_url = optional_env("LIVY_EXPLORER_BASE_URL");
        let program_id = optional_env("LIVY_RESOLVER_PROGRAM_ID");
        let bootstrap_template = env_bool("LIVY_PROVENANCE_BOOTSTRAP_TEMPLATE")?.unwrap_or(false);

        if !matches!(visibility.as_str(), "public" | "private") {
            return Err(ProvenanceError::InvalidEnv(
                "LIVY_PROVENANCE_VISIBILITY must be public or private".to_string(),
            ));
        }

        let livy = Livy::from_env().map_err(|err| ProvenanceError::InvalidEnv(err.to_string()))?;

        Ok(Some(Self {
            config: ProvenanceConfig {
                backend_base_url: trim_trailing_slash(&backend_base_url),
                explorer_base_url: explorer_base_url.map(|value| trim_trailing_slash(&value)),
                api_key,
                integration_id,
                schema_id,
                schema_version,
                visibility,
                verification_mode,
                subject_prefix,
                program_id,
                bootstrap_template,
            },
            http: reqwest::Client::new(),
            livy,
            template_ready: AtomicBool::new(false),
        }))
    }

    pub async fn attest_fetch(
        &self,
        evidence: ResolverFetchEvidence<'_>,
    ) -> Result<ProvenanceResult, ProvenanceError> {
        self.ensure_template_if_configured().await?;

        let fetched_at_unix_ms = unix_millis()?;
        let source = primary_source(evidence.payload);
        let data_sha256 = sha256_json_hex(evidence.data)?;
        let data_bytes = serde_json::to_vec(evidence.data)?.len();
        let input_commitment = request_summary(evidence.payload, evidence.route, evidence.mode);
        let output_commitment = json!({
            "kind": "spider_response",
            "sha256": data_sha256,
            "bytes": data_bytes,
        });
        let receipt_id = evidence.receipt.map(|receipt| receipt.id.clone());
        let subject_id = self.subject_id(
            &source,
            evidence.route,
            evidence.mode,
            receipt_id.as_deref(),
            fetched_at_unix_ms,
        )?;

        let mut builder = self.livy.attest();
        builder
            .commit(&APPLICATION_DOMAIN)
            .commit(&subject_id)
            .commit_hashed(&input_commitment)
            .commit_hashed(&output_commitment)
            .commit(&source)
            .commit(&evidence.route.as_str())
            .commit(&mode_label(evidence.mode))
            .commit(&fetched_at_unix_ms);
        if let Some(receipt_id) = receipt_id.as_ref() {
            builder.commit(receipt_id);
        }
        if let Some(program_id) = self.config.program_id.as_ref() {
            builder.commit(program_id);
        }
        builder.nonce(nonce_from_subject(&subject_id));

        let attestation = builder
            .finalize()
            .await
            .map_err(|err| ProvenanceError::Attestation(err.to_string()))?;
        let fields = self.schema_fields(
            &subject_id,
            &input_commitment,
            &output_commitment,
            &source,
            evidence.route,
            evidence.mode,
            fetched_at_unix_ms,
            receipt_id.as_deref(),
        );

        let request = json!({
            "integration_id": self.config.integration_id,
            "attestation_claim": ATTESTATION_CLAIM,
            "subject_type": SUBJECT_TYPE,
            "subject_id": subject_id,
            "schema_id": self.config.schema_id,
            "schema_version": self.config.schema_version,
            "visibility": self.config.visibility,
            "verification_mode": self.config.verification_mode,
            "attestation": attestation,
            "fields": fields,
            "metadata": {
                "application_domain": APPLICATION_DOMAIN,
                "resolver_service": "livy-resolver",
                "resolver_version": env!("CARGO_PKG_VERSION"),
                "route": evidence.route.as_str(),
                "mode": mode_label(evidence.mode),
                "source_sha256": sha256_string_hex(&source),
                "input_sha256": sha256_json_hex(&input_commitment)?,
                "output_sha256": data_sha256,
                "output_commitment_sha256": sha256_json_hex(&output_commitment)?,
                "receipt_id": receipt_id,
            },
        });

        let record: ProvenanceRecordResponse = self
            .http
            .post(self.endpoint("/api/v1/provenance/attestations"))
            .headers(self.headers()?)
            .json(&request)
            .send()
            .await?
            .error_for_status()
            .map_err(ProvenanceError::Http)?
            .json()
            .await?;

        Ok(ProvenanceResult {
            provenance_attestation_id: record.provenance_attestation_id.clone(),
            subject_id: record.subject_id,
            schema_id: self.config.schema_id.clone(),
            schema_version: self.config.schema_version.clone(),
            verification_status: record.verification_status,
            schema_binding_status: record.schema_binding_status,
            public_values_commitment: record.public_values_commitment,
            report_payload_hash: record.report_payload_hash,
            explorer_url: self.explorer_url(&record.provenance_attestation_id),
            data_sha256,
        })
    }

    fn subject_id(
        &self,
        source: &str,
        route: ProductRoute,
        mode: ProductMode,
        receipt_id: Option<&str>,
        fetched_at_unix_ms: u64,
    ) -> Result<String, ProvenanceError> {
        let material = json!({
            "source": source,
            "route": route.as_str(),
            "mode": mode_label(mode),
            "receipt_id": receipt_id,
            "fetched_at_unix_ms": fetched_at_unix_ms,
        });
        let digest = sha256_json_hex(&material)?;
        Ok(format!("{}:{}", self.config.subject_prefix, &digest[..24]))
    }

    fn schema_fields(
        &self,
        subject_id: &str,
        input_commitment: &Value,
        output_commitment: &Value,
        source: &str,
        route: ProductRoute,
        mode: ProductMode,
        fetched_at_unix_ms: u64,
        receipt_id: Option<&str>,
    ) -> Vec<Value> {
        let mut fields = vec![
            schema_field(
                0,
                "application_domain",
                "json",
                json!(APPLICATION_DOMAIN),
                true,
            ),
            schema_field(1, "subject_id", "json", json!(subject_id), true),
            schema_field(
                2,
                "input_commitment",
                "json_sha256",
                input_commitment.clone(),
                true,
            ),
            schema_field(
                3,
                "output_commitment",
                "json_sha256",
                output_commitment.clone(),
                true,
            ),
            schema_field(4, "source", "json", json!(source), true),
            schema_field(5, "route", "json", json!(route.as_str()), true),
            schema_field(6, "mode", "json", json!(mode_label(mode)), true),
            schema_field(
                7,
                "fetched_at_unix_ms",
                "json",
                json!(fetched_at_unix_ms),
                true,
            ),
        ];
        if let Some(receipt_id) = receipt_id {
            fields.push(schema_field(
                8,
                "receipt_id",
                "json",
                json!(receipt_id),
                false,
            ));
        }
        if let Some(program_id) = self.config.program_id.as_ref() {
            fields.push(schema_field(
                9,
                "program_id",
                "json",
                json!(program_id),
                false,
            ));
        }
        fields
    }

    async fn ensure_template_if_configured(&self) -> Result<(), ProvenanceError> {
        if !self.config.bootstrap_template || self.template_ready.load(Ordering::Acquire) {
            return Ok(());
        }

        let request = json!({
            "integration_id": self.config.integration_id,
            "schema_id": self.config.schema_id,
            "schema_version": self.config.schema_version,
            "attestation_claim": ATTESTATION_CLAIM,
            "subject_type": SUBJECT_TYPE,
            "name": "Resolver fetch",
            "template_kind": "resolver_source",
            "description": "Generic Livy resolver source-fetch proof. Use a separate prediction-market template only when the resolver emits market outcome fields.",
            "visibility": self.config.visibility,
            "fields": resolver_template_fields(),
            "metadata": {
                "application_domain": APPLICATION_DOMAIN,
                "resolver_service": "livy-resolver",
                "resolver_version": env!("CARGO_PKG_VERSION"),
            },
        });

        let response = self
            .http
            .post(self.endpoint("/api/v1/provenance/templates"))
            .headers(self.headers()?)
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(ProvenanceError::Backend(format!(
                "template bootstrap returned {status}: {body}"
            )));
        }

        self.template_ready.store(true, Ordering::Release);
        Ok(())
    }

    fn endpoint(&self, path: &str) -> String {
        format!("{}{}", self.config.backend_base_url, path)
    }

    fn headers(&self) -> Result<HeaderMap, ProvenanceError> {
        let mut headers = HeaderMap::new();
        let auth = HeaderValue::from_str(&format!("Bearer {}", self.config.api_key))
            .map_err(|err| ProvenanceError::InvalidEnv(err.to_string()))?;
        let integration = HeaderValue::from_str(&self.config.integration_id)
            .map_err(|err| ProvenanceError::InvalidEnv(err.to_string()))?;
        headers.insert(AUTHORIZATION, auth);
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert("x-integration-id", integration);
        Ok(headers)
    }

    fn explorer_url(&self, provenance_attestation_id: &str) -> Option<String> {
        let base = self
            .config
            .explorer_base_url
            .as_ref()
            .unwrap_or(&self.config.backend_base_url);
        Some(format!(
            "{base}/api/v1/public/provenance/attestations/{provenance_attestation_id}"
        ))
    }
}

pub(crate) fn request_summary(
    payload: &ProductRequest,
    route: ProductRoute,
    mode: ProductMode,
) -> Value {
    let mut summary = serde_json::Map::new();
    insert_value(&mut summary, "route", json!(route.as_str()));
    insert_value(&mut summary, "mode", json!(mode_label(mode)));
    insert_serialized(&mut summary, "source", &payload.source);
    insert_serialized(&mut summary, "query", &payload.query);
    insert_value(
        &mut summary,
        "format",
        serde_json::to_value(format_selection_value(payload.format.as_ref()))
            .unwrap_or(Value::Null),
    );
    insert_serialized(&mut summary, "formats", &payload.formats);
    insert_value(&mut summary, "proxy", json!(proxy_label(payload.proxy)));
    insert_serialized(&mut summary, "limit", &payload.limit);
    insert_serialized(&mut summary, "depth", &payload.depth);
    insert_serialized(&mut summary, "timeout_secs", &payload.timeout_secs);
    insert_serialized(
        &mut summary,
        "request_timeout_secs",
        &payload.request_timeout_secs,
    );
    insert_serialized(
        &mut summary,
        "crawl_timeout_secs",
        &payload.crawl_timeout_secs,
    );
    insert_serialized(&mut summary, "readability", &payload.readability);
    insert_serialized(&mut summary, "cache", &payload.cache);
    insert_serialized(&mut summary, "metadata", &payload.metadata);
    insert_serialized(&mut summary, "return_headers", &payload.return_headers);
    insert_serialized(
        &mut summary,
        "return_page_links",
        &payload.return_page_links,
    );
    insert_serialized(&mut summary, "return_cookies", &payload.return_cookies);
    insert_serialized(&mut summary, "country_code", &payload.country_code);
    insert_serialized(&mut summary, "locale", &payload.locale);
    insert_value(
        &mut summary,
        "user_agent_present",
        json!(payload.user_agent.as_ref().map(|value| !value.is_empty())),
    );
    insert_value(
        &mut summary,
        "header_count",
        json!(payload.headers.as_ref().map(|headers| headers.len())),
    );
    insert_value(
        &mut summary,
        "cookies_present",
        json!(payload.cookies.as_ref().map(|value| !value.is_empty())),
    );
    insert_serialized(&mut summary, "root_selector", &payload.root_selector);
    insert_serialized(&mut summary, "selectors", &payload.selectors);
    insert_serialized(&mut summary, "whitelist", &payload.whitelist);
    insert_serialized(&mut summary, "blacklist", &payload.blacklist);
    insert_serialized(&mut summary, "tld", &payload.tld);
    insert_serialized(&mut summary, "subdomains", &payload.subdomains);
    insert_serialized(&mut summary, "external_domains", &payload.external_domains);
    insert_serialized(&mut summary, "sitemap", &payload.sitemap);
    insert_serialized(&mut summary, "respect_robots", &payload.respect_robots);
    insert_serialized(&mut summary, "stealth", &payload.stealth);
    insert_serialized(&mut summary, "fingerprint", &payload.fingerprint);
    insert_serialized(&mut summary, "scroll", &payload.scroll);
    insert_serialized(&mut summary, "wait_ms", &payload.wait_ms);
    insert_serialized(&mut summary, "wait_selector", &payload.wait_selector);
    insert_serialized(
        &mut summary,
        "disable_intercept",
        &payload.disable_intercept,
    );
    insert_serialized(&mut summary, "disable_hints", &payload.disable_hints);
    insert_serialized(&mut summary, "lite_mode", &payload.lite_mode);
    insert_serialized(
        &mut summary,
        "max_credits_per_page",
        &payload.max_credits_per_page,
    );
    insert_serialized(&mut summary, "receipt", &payload.receipt);
    insert_serialized(&mut summary, "search_limit", &payload.search_limit);
    insert_serialized(
        &mut summary,
        "fetch_page_content",
        &payload.fetch_page_content,
    );
    insert_serialized(&mut summary, "quick_search", &payload.quick_search);
    insert_serialized(&mut summary, "engine", &payload.engine);
    Value::Object(summary)
}

pub(crate) fn sha256_json_hex<T: Serialize>(value: &T) -> Result<String, ProvenanceError> {
    let encoded = serde_json::to_vec(value)?;
    Ok(sha256_bytes_hex(&encoded))
}

fn insert_serialized<T: Serialize>(map: &mut serde_json::Map<String, Value>, key: &str, value: &T) {
    insert_value(map, key, serde_json::to_value(value).unwrap_or(Value::Null));
}

fn insert_value(map: &mut serde_json::Map<String, Value>, key: &str, value: Value) {
    map.insert(key.to_string(), value);
}

fn sha256_string_hex(value: &str) -> String {
    sha256_bytes_hex(value.as_bytes())
}

fn sha256_bytes_hex(bytes: &[u8]) -> String {
    let digest: [u8; 32] = Sha256::digest(bytes).into();
    hex::encode(digest)
}

fn nonce_from_subject(subject_id: &str) -> u64 {
    let digest: [u8; 32] = Sha256::digest(subject_id.as_bytes()).into();
    u64::from_le_bytes([
        digest[0], digest[1], digest[2], digest[3], digest[4], digest[5], digest[6], digest[7],
    ])
}

fn schema_field(index: u32, name: &str, commit_mode: &str, value: Value, required: bool) -> Value {
    json!({
        "index": index,
        "name": name,
        "commit_mode": commit_mode,
        "value": value,
        "required": required,
    })
}

fn resolver_template_fields() -> Vec<Value> {
    vec![
        template_field(
            0,
            "application_domain",
            "json",
            "public",
            true,
            "string",
            "Static application domain for resolver proofs.",
        ),
        template_field(
            1,
            "subject_id",
            "json",
            "public",
            true,
            "string",
            "Unique resolver fetch subject.",
        ),
        template_field(
            2,
            "input_commitment",
            "json_sha256",
            "commitment",
            true,
            "object",
            "Redacted request summary committed by hash.",
        ),
        template_field(
            3,
            "output_commitment",
            "json_sha256",
            "commitment",
            true,
            "object",
            "Hash and byte length of the upstream resolver output.",
        ),
        template_field(
            4,
            "source",
            "json",
            "public",
            true,
            "string",
            "Fetched URL or search query.",
        ),
        template_field(
            5,
            "route",
            "json",
            "public",
            true,
            "string",
            "Resolver HTTP or MCP route.",
        ),
        template_field(
            6,
            "mode",
            "json",
            "public",
            true,
            "string",
            "Resolved execution mode.",
        ),
        template_field(
            7,
            "fetched_at_unix_ms",
            "json",
            "public",
            true,
            "integer",
            "Fetch timestamp.",
        ),
        template_field(
            8,
            "receipt_id",
            "json",
            "public",
            false,
            "string",
            "Resolver receipt id when created.",
        ),
        template_field(
            9,
            "program_id",
            "json",
            "public",
            false,
            "string",
            "Optional deployment or program identifier.",
        ),
    ]
}

fn template_field(
    index: u32,
    name: &str,
    commit_mode: &str,
    disclosure: &str,
    required: bool,
    value_type: &str,
    description: &str,
) -> Value {
    json!({
        "index": index,
        "name": name,
        "commit_mode": commit_mode,
        "disclosure": disclosure,
        "required": required,
        "value_type": value_type,
        "description": description,
    })
}

fn primary_source(payload: &ProductRequest) -> String {
    payload
        .source
        .as_deref()
        .or(payload.query.as_deref())
        .unwrap_or("unknown")
        .to_string()
}

fn format_selection_value(format: Option<&FormatSelection>) -> Option<Value> {
    match format {
        Some(FormatSelection::One(format)) => Some(json!(product_format_label(*format))),
        Some(FormatSelection::Many(formats)) => Some(json!(
            formats
                .iter()
                .map(|format| product_format_label(*format))
                .collect::<Vec<_>>()
        )),
        None => None,
    }
}

fn product_format_label(format: ProductFormat) -> &'static str {
    match format {
        ProductFormat::Raw => "raw",
        ProductFormat::Markdown => "markdown",
        ProductFormat::Commonmark => "commonmark",
        ProductFormat::Html2text => "html2text",
        ProductFormat::Text => "text",
        ProductFormat::Screenshot => "screenshot",
        ProductFormat::Xml => "xml",
        ProductFormat::Bytes => "bytes",
    }
}

fn proxy_label(proxy: Option<ProductProxy>) -> Option<&'static str> {
    proxy.map(|proxy| match proxy {
        ProductProxy::Auto => "auto",
        ProductProxy::None => "none",
        ProductProxy::Isp => "isp",
        ProductProxy::Residential => "residential",
        ProductProxy::Mobile => "mobile",
    })
}

fn mode_label(mode: ProductMode) -> &'static str {
    match mode {
        ProductMode::Auto => "auto",
        ProductMode::Fast => "fast",
        ProductMode::Browser => "browser",
        ProductMode::Unblock => "unblock",
        ProductMode::Raw => "raw",
        ProductMode::Crawl => "crawl",
        ProductMode::Map => "map",
        ProductMode::Search => "search",
        ProductMode::Extract => "extract",
        ProductMode::Screenshot => "screenshot",
    }
}

fn unix_millis() -> Result<u64, ProvenanceError> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| ProvenanceError::Time(err.to_string()))?
        .as_millis();
    u64::try_from(millis).map_err(|err| ProvenanceError::Time(err.to_string()))
}

fn required_env(name: &'static str) -> Result<String, ProvenanceError> {
    optional_env(name).ok_or(ProvenanceError::MissingEnv(name))
}

fn optional_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn env_or(name: &str, default: &str) -> String {
    optional_env(name).unwrap_or_else(|| default.to_string())
}

fn env_present(name: &str) -> bool {
    std::env::var(name)
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false)
}

fn env_bool(name: &str) -> Result<Option<bool>, ProvenanceError> {
    let Some(value) = optional_env(name) else {
        return Ok(None);
    };
    match value.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(Some(true)),
        "0" | "false" | "no" | "off" => Ok(Some(false)),
        _ => Err(ProvenanceError::InvalidEnv(format!(
            "{name} must be a boolean"
        ))),
    }
}

fn trim_trailing_slash(value: &str) -> String {
    value.trim_end_matches('/').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn request_summary_redacts_headers_and_cookies() {
        let mut payload = ProductRequest::fast("https://example.com/path");
        payload.headers = Some(HashMap::from([(
            "authorization".to_string(),
            "Bearer secret".to_string(),
        )]));
        payload.cookies = Some("session=secret".to_string());
        payload.user_agent = Some("private-agent".to_string());

        let summary = request_summary(&payload, ProductRoute::Scrape, ProductMode::Fast);
        let encoded = serde_json::to_string(&summary).unwrap();

        assert!(encoded.contains("\"header_count\":1"));
        assert!(encoded.contains("\"cookies_present\":true"));
        assert!(encoded.contains("\"user_agent_present\":true"));
        assert!(!encoded.contains("Bearer secret"));
        assert!(!encoded.contains("session=secret"));
        assert!(!encoded.contains("private-agent"));
    }

    #[test]
    fn resolver_template_uses_generic_source_claim() {
        let fields = resolver_template_fields();

        assert_eq!(fields[0]["name"], "application_domain");
        assert_eq!(fields[2]["commit_mode"], "json_sha256");
        assert_eq!(fields[3]["disclosure"], "commitment");
        assert_eq!(ATTESTATION_CLAIM, "source");
        assert_eq!(SUBJECT_TYPE, "resolver_fetch");
        assert_eq!(DEFAULT_SCHEMA_ID, "resolver-fetch-v1");
    }
}
