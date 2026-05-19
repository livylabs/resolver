//! Product-facing request, route, mode, and receipt types.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// High-level route behavior exposed to API clients.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProductMode {
    /// Let the service choose the default SmartMode path.
    #[serde(alias = "smart", alias = "smart_mode")]
    Auto,
    /// Use the fast SmartMode + ISP proxy source-fetch path.
    Fast,
    /// Use browser rendering for JavaScript-heavy pages.
    Browser,
    /// Use browser rendering with stealth and proxy defaults.
    Unblock,
    /// Use a plain HTTP request without readability cleanup.
    Raw,
    /// Crawl a website with depth and limit controls.
    Crawl,
    /// Return discovered links without fetching every page.
    Map,
    /// Search the web and optionally fetch result pages.
    Search,
    /// Fetch content prepared for downstream structured extraction.
    Extract,
    /// Return a page screenshot payload.
    Screenshot,
}

impl Default for ProductMode {
    fn default() -> Self {
        Self::Auto
    }
}

impl ProductMode {
    /// Stable receipt label for the selected mode.
    pub fn receipt_label(self) -> &'static str {
        match self {
            Self::Auto => "fetch:auto",
            Self::Fast => "fetch:fast",
            Self::Browser => "fetch:browser",
            Self::Unblock => "fetch:unblock",
            Self::Raw => "fetch:raw",
            Self::Crawl => "crawl",
            Self::Map => "map",
            Self::Search => "search",
            Self::Extract => "extract",
            Self::Screenshot => "screenshot",
        }
    }
}

/// Output formats clients can request from Spider.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum ProductFormat {
    /// Return raw source content.
    Raw,
    /// Return markdown content.
    Markdown,
    /// Return CommonMark content.
    Commonmark,
    /// Return html2text content.
    Html2text,
    /// Return plain text content.
    Text,
    /// Return a screenshot payload.
    Screenshot,
    /// Return XML content.
    Xml,
    /// Return bytes content.
    Bytes,
}

/// Single or multiple output format selection.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum FormatSelection {
    /// One requested format.
    One(ProductFormat),
    /// Multiple requested formats.
    Many(Vec<ProductFormat>),
}

/// Proxy policy requested by the caller.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProductProxy {
    /// Let the route choose a proxy default.
    Auto,
    /// Do not request a proxy.
    None,
    /// Use ISP proxy routing.
    Isp,
    /// Use residential proxy routing.
    Residential,
    /// Use mobile proxy routing.
    Mobile,
}

/// Product API request accepted by route handlers.
#[derive(Clone, Debug, Deserialize)]
pub struct ProductRequest {
    /// Exact URL to fetch, crawl, map, extract, or screenshot.
    #[serde(alias = "url")]
    pub source: Option<String>,
    /// Search query for the search route.
    #[serde(alias = "q")]
    pub query: Option<String>,
    /// High-level execution mode.
    #[serde(default)]
    pub mode: ProductMode,
    /// Output format as one value or an array.
    pub format: Option<FormatSelection>,
    /// Output formats as an explicit array.
    pub formats: Option<Vec<ProductFormat>>,
    /// Proxy routing preference.
    pub proxy: Option<ProductProxy>,
    /// Maximum page or result count.
    pub limit: Option<u32>,
    /// Crawl depth.
    pub depth: Option<u32>,
    /// Outer service timeout in seconds.
    pub timeout_secs: Option<u64>,
    /// Per-request upstream timeout in seconds.
    pub request_timeout_secs: Option<u8>,
    /// Upstream crawl timeout in seconds.
    pub crawl_timeout_secs: Option<u64>,
    /// Enable readability cleanup.
    pub readability: Option<bool>,
    /// Allow upstream caching.
    pub cache: Option<bool>,
    /// Return metadata in upstream payloads.
    pub metadata: Option<bool>,
    /// Return response headers.
    pub return_headers: Option<bool>,
    /// Return discovered page links.
    pub return_page_links: Option<bool>,
    /// Return response cookies.
    pub return_cookies: Option<bool>,
    /// Country code for request routing.
    pub country_code: Option<String>,
    /// Locale for browser or request hints.
    pub locale: Option<String>,
    /// Custom user agent.
    pub user_agent: Option<String>,
    /// Custom request headers.
    pub headers: Option<HashMap<String, String>>,
    /// Request cookies.
    pub cookies: Option<String>,
    /// Root CSS selector for content filtering.
    pub root_selector: Option<String>,
    /// Named CSS selectors for extraction.
    pub selectors: Option<HashMap<String, Vec<String>>>,
    /// URL allow-list patterns.
    pub whitelist: Option<Vec<String>>,
    /// URL block-list patterns.
    pub blacklist: Option<Vec<String>>,
    /// Restrict crawling to top-level domain.
    pub tld: Option<bool>,
    /// Include subdomains while crawling.
    pub subdomains: Option<bool>,
    /// External domains allowed during crawl.
    pub external_domains: Option<Vec<String>>,
    /// Use sitemap discovery.
    pub sitemap: Option<bool>,
    /// Respect robots.txt.
    pub respect_robots: Option<bool>,
    /// Enable stealth browser behavior.
    pub stealth: Option<bool>,
    /// Enable browser fingerprint handling.
    pub fingerprint: Option<bool>,
    /// Number of scroll steps.
    pub scroll: Option<u32>,
    /// Hard wait in milliseconds.
    pub wait_ms: Option<u64>,
    /// Selector to wait for.
    pub wait_selector: Option<String>,
    /// Disable request interception.
    pub disable_intercept: Option<bool>,
    /// Disable upstream optimization hints.
    pub disable_hints: Option<bool>,
    /// Use lower-cost lite mode.
    pub lite_mode: Option<bool>,
    /// Maximum upstream credits per page.
    pub max_credits_per_page: Option<f64>,
    /// Whether to create an in-memory receipt.
    pub receipt: Option<bool>,
    /// Number of search results to request.
    pub search_limit: Option<u32>,
    /// Fetch content for search results.
    pub fetch_page_content: Option<bool>,
    /// Prefer faster search results.
    pub quick_search: Option<bool>,
    /// Search engine selector.
    pub engine: Option<String>,
}

impl ProductRequest {
    /// Legacy exact-source request used by older handlers.
    pub fn legacy(source: &str) -> Self {
        Self {
            source: Some(source.to_string()),
            query: None,
            mode: ProductMode::Auto,
            format: None,
            formats: None,
            proxy: Some(ProductProxy::None),
            limit: None,
            depth: None,
            timeout_secs: Some(25),
            request_timeout_secs: Some(20),
            crawl_timeout_secs: Some(25),
            readability: Some(true),
            cache: None,
            metadata: None,
            return_headers: None,
            return_page_links: None,
            return_cookies: None,
            country_code: None,
            locale: None,
            user_agent: None,
            headers: None,
            cookies: None,
            root_selector: None,
            selectors: None,
            whitelist: None,
            blacklist: None,
            tld: None,
            subdomains: None,
            external_domains: None,
            sitemap: None,
            respect_robots: None,
            stealth: None,
            fingerprint: None,
            scroll: None,
            wait_ms: None,
            wait_selector: None,
            disable_intercept: None,
            disable_hints: None,
            lite_mode: None,
            max_credits_per_page: None,
            receipt: Some(false),
            search_limit: None,
            fetch_page_content: None,
            quick_search: None,
            engine: None,
        }
    }

    /// Fast source request used by MCP and receipt-backed fetches.
    pub fn fast(source: &str) -> Self {
        Self {
            mode: ProductMode::Fast,
            proxy: Some(ProductProxy::Isp),
            receipt: Some(true),
            ..Self::legacy(source)
        }
    }

    /// Return the request URL or a clear API error.
    pub fn require_source(&self) -> Result<&str, crate::errors::FetchError> {
        self.source
            .as_deref()
            .ok_or_else(|| crate::errors::FetchError::BadRequest("route requires `source`".into()))
    }
}

/// Internal route selected by each HTTP handler.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProductRoute {
    /// One-page scrape or fetch.
    Scrape,
    /// Website crawl.
    Crawl,
    /// Link discovery.
    Map,
    /// Web search.
    Search,
    /// Fetch for structured extraction.
    Extract,
    /// Screenshot capture.
    Screenshot,
    /// Stealth unblock fetch.
    Unblock,
}

impl ProductRoute {
    /// Public route name included in responses.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Scrape => "fetch",
            Self::Crawl => "crawl",
            Self::Map => "map",
            Self::Search => "search",
            Self::Extract => "extract",
            Self::Screenshot => "screenshot",
            Self::Unblock => "unblock",
        }
    }
}

/// In-memory provenance metadata for receipt-backed requests.
#[derive(Clone, Serialize)]
pub struct Receipt {
    /// Receipt identifier.
    pub id: String,
    /// Original URL or query.
    pub source_url: String,
    /// Product mode label.
    pub mode: String,
    /// Upstream request type label.
    pub request_type: String,
    /// Proxy label when one was used.
    pub proxy: Option<String>,
    /// Upstream status value.
    pub status: Option<i64>,
    /// Upstream error text.
    pub error: Option<String>,
    /// Upstream elapsed time in milliseconds.
    pub duration_elapsed_ms: Option<u64>,
    /// Extracted content byte length.
    pub content_bytes: Option<usize>,
    /// Upstream formatted total cost.
    pub total_cost: Option<String>,
    /// Receipt creation time.
    pub created_at_unix_ms: u128,
    /// Human-readable demo marker.
    pub demo_message: String,
}

/// Legacy receipt-backed fetch response.
#[derive(Serialize)]
pub struct FetchWithReceipt {
    /// Receipt identifier.
    pub receipt_id: String,
    /// Full receipt metadata.
    pub receipt: Receipt,
    /// Upstream payload.
    pub data: Value,
}

/// Product route response envelope.
#[derive(Serialize)]
pub struct ProductResponse {
    /// Executed route name.
    pub route: String,
    /// Resolved product mode.
    pub mode: ProductMode,
    /// Receipt identifier when created.
    pub receipt_id: Option<String>,
    /// Full receipt metadata when created.
    pub receipt: Option<Receipt>,
    /// Upstream payload.
    pub data: Value,
}
