//! Livy provenance client for resolver fetch attestations.

use crate::types::{
    FormatSelection, ProductFormat, ProductMode, ProductProxy, ProductRequest, ProductRoute,
    Receipt,
};
use livy_provenance_sdk::{
    CreateProvenanceAttestationRequest, DEFAULT_LIVY_API_BASE_URL, ProvenanceAttestationField,
    ProvenanceClient as LivyProvenanceApiClient, ProvenanceClientConfig, ProvenanceClientError,
    ProvenanceCommitMode, ProvenanceFieldDisclosure, ProvenanceTemplateField,
    ProvenanceVerificationMode, UpsertProvenanceTemplateRequest,
};
use livy_tee::Livy;
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
    api: LivyProvenanceApiClient,
    livy: Livy,
    template_ready: AtomicBool,
}

#[derive(Debug, Clone)]
struct ProvenanceConfig {
    backend_base_url: String,
    explorer_base_url: Option<String>,
    schema_id: String,
    schema_version: String,
    visibility: String,
    verification_mode: ProvenanceVerificationMode,
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
    Sdk(ProvenanceClientError),
    Json(serde_json::Error),
    Attestation(String),
    Time(String),
}

impl fmt::Display for ProvenanceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingEnv(name) => write!(f, "{name} must be set when provenance is enabled"),
            Self::InvalidEnv(message) => write!(f, "invalid provenance configuration: {message}"),
            Self::Sdk(err) => write!(f, "provenance SDK failed: {err}"),
            Self::Json(err) => write!(f, "provenance JSON handling failed: {err}"),
            Self::Attestation(err) => write!(f, "provenance attestation failed: {err}"),
            Self::Time(err) => write!(f, "provenance timestamp failed: {err}"),
        }
    }
}

impl std::error::Error for ProvenanceError {}

impl From<ProvenanceClientError> for ProvenanceError {
    fn from(err: ProvenanceClientError) -> Self {
        Self::Sdk(err)
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

impl ProvenanceClient {
    pub fn from_env() -> Result<Option<Self>, ProvenanceError> {
        let configured = env_present("LIVY_BACKEND_BASE_URL")
            || env_present("LIVY_API_KEY")
            || env_present("LIVY_PROVENANCE_ENABLED");
        let enabled = env_bool("LIVY_PROVENANCE_ENABLED")?.unwrap_or(configured);

        if !enabled {
            return Ok(None);
        }

        let backend_base_url = backend_base_url_from_env();
        let api_key = required_env("LIVY_API_KEY")?;
        let integration_id = env_or("LIVY_INTEGRATION_ID", DEFAULT_INTEGRATION_ID);
        let schema_id = env_or("LIVY_PROVENANCE_SCHEMA_ID", DEFAULT_SCHEMA_ID);
        let schema_version = env_or("LIVY_PROVENANCE_SCHEMA_VERSION", DEFAULT_SCHEMA_VERSION);
        let visibility = env_or("LIVY_PROVENANCE_VISIBILITY", "public");
        let verification_mode =
            parse_verification_mode(&env_or("LIVY_PROVENANCE_VERIFICATION_MODE", "verify_fresh"))?;
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
        let api = LivyProvenanceApiClient::new(ProvenanceClientConfig::with_base_url(
            backend_base_url.clone(),
            api_key,
            integration_id,
        ))?;

        Ok(Some(Self {
            config: ProvenanceConfig {
                backend_base_url,
                explorer_base_url: explorer_base_url.map(|value| trim_trailing_slash(&value)),
                schema_id,
                schema_version,
                visibility,
                verification_mode,
                subject_prefix,
                program_id,
                bootstrap_template,
            },
            api,
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

        let request = CreateProvenanceAttestationRequest {
            attestation_claim: ATTESTATION_CLAIM.to_string(),
            subject_type: SUBJECT_TYPE.to_string(),
            subject_id,
            schema_id: Some(self.config.schema_id.clone()),
            schema_version: Some(self.config.schema_version.clone()),
            visibility: self.config.visibility.clone(),
            verification_mode: Some(self.config.verification_mode),
            attestation: serde_json::to_value(attestation)?,
            fields,
            metadata: json!({
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
            }),
        };

        let record = self.api.create_attestation(request).await?;

        Ok(ProvenanceResult {
            provenance_attestation_id: record.provenance_attestation_id.clone(),
            subject_id: record.subject_id,
            schema_id: record
                .schema_id
                .unwrap_or_else(|| self.config.schema_id.clone()),
            schema_version: record
                .schema_version
                .unwrap_or_else(|| self.config.schema_version.clone()),
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
    ) -> Vec<ProvenanceAttestationField> {
        let mut fields = vec![
            schema_field(
                0,
                "application_domain",
                ProvenanceCommitMode::Json,
                json!(APPLICATION_DOMAIN),
                true,
            ),
            schema_field(
                1,
                "subject_id",
                ProvenanceCommitMode::Json,
                json!(subject_id),
                true,
            ),
            schema_field(
                2,
                "input_commitment",
                ProvenanceCommitMode::JsonSha256,
                input_commitment.clone(),
                true,
            ),
            schema_field(
                3,
                "output_commitment",
                ProvenanceCommitMode::JsonSha256,
                output_commitment.clone(),
                true,
            ),
            schema_field(4, "source", ProvenanceCommitMode::Json, json!(source), true),
            schema_field(
                5,
                "route",
                ProvenanceCommitMode::Json,
                json!(route.as_str()),
                true,
            ),
            schema_field(
                6,
                "mode",
                ProvenanceCommitMode::Json,
                json!(mode_label(mode)),
                true,
            ),
            schema_field(
                7,
                "fetched_at_unix_ms",
                ProvenanceCommitMode::Json,
                json!(fetched_at_unix_ms),
                true,
            ),
        ];
        if let Some(receipt_id) = receipt_id {
            fields.push(schema_field(
                8,
                "receipt_id",
                ProvenanceCommitMode::Json,
                json!(receipt_id),
                false,
            ));
        }
        if let Some(program_id) = self.config.program_id.as_ref() {
            fields.push(schema_field(
                9,
                "program_id",
                ProvenanceCommitMode::Json,
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

        let request = UpsertProvenanceTemplateRequest {
            schema_id: self.config.schema_id.clone(),
            schema_version: self.config.schema_version.clone(),
            attestation_claim: ATTESTATION_CLAIM.to_string(),
            subject_type: SUBJECT_TYPE.to_string(),
            name: "Resolver fetch".to_string(),
            template_kind: "resolver_source".to_string(),
            description: "Generic Livy resolver source-fetch proof. Use a separate prediction-market template only when the resolver emits market outcome fields.".to_string(),
            visibility: self.config.visibility.clone(),
            fields: resolver_template_fields(),
            metadata: json!({
                "application_domain": APPLICATION_DOMAIN,
                "resolver_service": "livy-resolver",
                "resolver_version": env!("CARGO_PKG_VERSION"),
            }),
        };

        self.api.upsert_template(request).await?;

        self.template_ready.store(true, Ordering::Release);
        Ok(())
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

fn schema_field(
    index: u32,
    name: &str,
    commit_mode: ProvenanceCommitMode,
    value: Value,
    required: bool,
) -> ProvenanceAttestationField {
    ProvenanceAttestationField {
        index,
        name: name.to_string(),
        commit_mode,
        value,
        required,
    }
}

fn resolver_template_fields() -> Vec<ProvenanceTemplateField> {
    vec![
        template_field(
            0,
            "application_domain",
            ProvenanceCommitMode::Json,
            ProvenanceFieldDisclosure::Public,
            true,
            "string",
            "Static application domain for resolver proofs.",
        ),
        template_field(
            1,
            "subject_id",
            ProvenanceCommitMode::Json,
            ProvenanceFieldDisclosure::Public,
            true,
            "string",
            "Unique resolver fetch subject.",
        ),
        template_field(
            2,
            "input_commitment",
            ProvenanceCommitMode::JsonSha256,
            ProvenanceFieldDisclosure::Commitment,
            true,
            "object",
            "Redacted request summary committed by hash.",
        ),
        template_field(
            3,
            "output_commitment",
            ProvenanceCommitMode::JsonSha256,
            ProvenanceFieldDisclosure::Commitment,
            true,
            "object",
            "Hash and byte length of the upstream resolver output.",
        ),
        template_field(
            4,
            "source",
            ProvenanceCommitMode::Json,
            ProvenanceFieldDisclosure::Public,
            true,
            "string",
            "Fetched URL or search query.",
        ),
        template_field(
            5,
            "route",
            ProvenanceCommitMode::Json,
            ProvenanceFieldDisclosure::Public,
            true,
            "string",
            "Resolver HTTP or MCP route.",
        ),
        template_field(
            6,
            "mode",
            ProvenanceCommitMode::Json,
            ProvenanceFieldDisclosure::Public,
            true,
            "string",
            "Resolved execution mode.",
        ),
        template_field(
            7,
            "fetched_at_unix_ms",
            ProvenanceCommitMode::Json,
            ProvenanceFieldDisclosure::Public,
            true,
            "integer",
            "Fetch timestamp.",
        ),
        template_field(
            8,
            "receipt_id",
            ProvenanceCommitMode::Json,
            ProvenanceFieldDisclosure::Public,
            false,
            "string",
            "Resolver receipt id when created.",
        ),
        template_field(
            9,
            "program_id",
            ProvenanceCommitMode::Json,
            ProvenanceFieldDisclosure::Public,
            false,
            "string",
            "Optional deployment or program identifier.",
        ),
    ]
}

fn template_field(
    index: u32,
    name: &str,
    commit_mode: ProvenanceCommitMode,
    disclosure: ProvenanceFieldDisclosure,
    required: bool,
    value_type: &str,
    description: &str,
) -> ProvenanceTemplateField {
    ProvenanceTemplateField {
        index,
        name: name.to_string(),
        commit_mode,
        disclosure,
        required,
        value_type: Some(value_type.to_string()),
        description: Some(description.to_string()),
    }
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

fn backend_base_url_from_env() -> String {
    configured_backend_base_url(optional_env("LIVY_BACKEND_BASE_URL"))
}

fn configured_backend_base_url(value: Option<String>) -> String {
    value
        .map(|value| trim_trailing_slash(&value))
        .unwrap_or_else(|| DEFAULT_LIVY_API_BASE_URL.to_string())
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

fn parse_verification_mode(value: &str) -> Result<ProvenanceVerificationMode, ProvenanceError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "binding_only" => Ok(ProvenanceVerificationMode::BindingOnly),
        "verify" => Ok(ProvenanceVerificationMode::Verify),
        "verify_fresh" => Ok(ProvenanceVerificationMode::VerifyFresh),
        _ => Err(ProvenanceError::InvalidEnv(
            "LIVY_PROVENANCE_VERIFICATION_MODE must be binding_only, verify, or verify_fresh"
                .to_string(),
        )),
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

        assert_eq!(fields[0].name, "application_domain");
        assert_eq!(fields[2].commit_mode, ProvenanceCommitMode::JsonSha256);
        assert_eq!(fields[3].disclosure, ProvenanceFieldDisclosure::Commitment);
        assert_eq!(ATTESTATION_CLAIM, "source");
        assert_eq!(SUBJECT_TYPE, "resolver_fetch");
        assert_eq!(DEFAULT_SCHEMA_ID, "resolver-fetch-v1");
    }

    #[test]
    fn verification_mode_env_values_map_to_sdk_enum() {
        assert_eq!(
            parse_verification_mode("binding_only").unwrap(),
            ProvenanceVerificationMode::BindingOnly
        );
        assert_eq!(
            parse_verification_mode("verify").unwrap(),
            ProvenanceVerificationMode::Verify
        );
        assert_eq!(
            parse_verification_mode("verify_fresh").unwrap(),
            ProvenanceVerificationMode::VerifyFresh
        );
        assert!(matches!(
            parse_verification_mode("live"),
            Err(ProvenanceError::InvalidEnv(_))
        ));
    }

    #[test]
    fn backend_base_url_defaults_to_livy_api_and_allows_override() {
        assert_eq!(configured_backend_base_url(None), DEFAULT_LIVY_API_BASE_URL);
        assert_eq!(
            configured_backend_base_url(Some("http://localhost:8081///".to_string())),
            "http://localhost:8081"
        );
    }
}
