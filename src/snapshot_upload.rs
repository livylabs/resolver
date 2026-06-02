use crate::fetch::Fetcher;
use ar_upload::{DataItem, EthereumSigner, Tag, TurboClient};
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD, URL_SAFE, URL_SAFE_NO_PAD};
use serde::Serialize;
use serde_json::Value;
use std::fmt;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

const APP_NAME: &str = "livy-resolver";

#[derive(Debug, Clone, Serialize)]
pub struct SnapshotPayload {
    pub source_url: String,
    pub receipt_id: String,
    pub html: String,
    pub screenshot_base64: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SnapshotUploadResult {
    pub html_id: String,
    pub screenshot_id: String,
}

#[derive(Debug)]
pub enum SnapshotError {
    MissingHtml,
    MissingScreenshot,
    InvalidScreenshotBase64(String),
    MissingPrivateKey,
    InvalidPrivateKey(String),
    Upload(String),
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SnapshotError::MissingHtml => {
                write!(f, "Spider snapshot response did not include raw HTML")
            }
            SnapshotError::MissingScreenshot => {
                write!(f, "Spider snapshot response did not include a screenshot")
            }
            SnapshotError::InvalidScreenshotBase64(e) => {
                write!(f, "invalid screenshot base64: {e}")
            }
            SnapshotError::MissingPrivateKey => write!(f, "AR_UPLOAD_ETH_PRIVATE_KEY is not set"),
            SnapshotError::InvalidPrivateKey(e) => write!(f, "invalid Ethereum private key: {e}"),
            SnapshotError::Upload(e) => write!(f, "snapshot upload failed: {e}"),
        }
    }
}

impl std::error::Error for SnapshotError {}

impl SnapshotPayload {
    pub fn from_spider_response(source_url: &str, response: Value) -> Result<Self, SnapshotError> {
        let html = find_string_by_keys(&response, &["raw", "html"])
            .or_else(|| find_content_string(&response))
            .ok_or(SnapshotError::MissingHtml)?;
        let screenshot_base64 = find_string_by_keys(&response, &["screenshot"])
            .ok_or(SnapshotError::MissingScreenshot)?;
        let receipt_id = find_string_by_keys(
            &response,
            &["receipt_id", "receiptId", "request_id", "requestId", "id"],
        )
        .unwrap_or_else(new_receipt_id);

        Ok(Self {
            source_url: source_url.to_string(),
            receipt_id,
            html,
            screenshot_base64,
        })
    }

    fn screenshot_bytes(&self) -> Result<Vec<u8>, SnapshotError> {
        decode_base64_image(&self.screenshot_base64)
    }
}

pub fn spawn_snapshot_upload(fetcher: Arc<Fetcher>, source_url: String) {
    tokio::spawn(async move {
        match fetcher.snapshot_with_receipt(&source_url).await {
            Ok(snapshot) => match upload_snapshot(snapshot).await {
                Ok(result) => {
                    eprintln!(
                        "snapshot uploaded source_url={} html_id={} screenshot_id={}",
                        source_url, result.html_id, result.screenshot_id
                    );
                }
                Err(err) => {
                    eprintln!("snapshot upload failed source_url={source_url}: {err}");
                }
            },
            Err(err) => {
                eprintln!("snapshot fetch failed source_url={source_url}: {err}");
            }
        }
    });
}

pub fn spawn_snapshot_payload_upload(snapshot: SnapshotPayload) {
    tokio::spawn(async move {
        let source_url = snapshot.source_url.clone();
        match upload_snapshot(snapshot).await {
            Ok(result) => {
                eprintln!(
                    "snapshot uploaded source_url={} html_id={} screenshot_id={}",
                    source_url, result.html_id, result.screenshot_id
                );
            }
            Err(err) => {
                eprintln!("snapshot upload failed source_url={source_url}: {err}");
            }
        }
    });
}

pub async fn upload_snapshot(
    snapshot: SnapshotPayload,
) -> Result<SnapshotUploadResult, SnapshotError> {
    let private_key = private_key_from_env()?;
    let signer = EthereumSigner::from_key(&private_key)
        .map_err(|e| SnapshotError::InvalidPrivateKey(e.to_string()))?;
    let client = TurboClient::new();
    let screenshot_bytes = snapshot.screenshot_bytes()?;

    let html_item = build_item(
        &signer,
        snapshot.html.into_bytes(),
        "text/html; charset=utf-8",
        "html",
        &snapshot.source_url,
        &snapshot.receipt_id,
    )
    .await?;
    let screenshot_item = build_item(
        &signer,
        screenshot_bytes,
        "image/png",
        "screenshot",
        &snapshot.source_url,
        &snapshot.receipt_id,
    )
    .await?;

    let html_resp = client
        .upload(&html_item)
        .await
        .map_err(|e| SnapshotError::Upload(e.to_string()))?;
    let screenshot_resp = client
        .upload(&screenshot_item)
        .await
        .map_err(|e| SnapshotError::Upload(e.to_string()))?;

    Ok(SnapshotUploadResult {
        html_id: html_resp.id,
        screenshot_id: screenshot_resp.id,
    })
}

async fn build_item(
    signer: &EthereumSigner,
    data: Vec<u8>,
    content_type: &str,
    snapshot_kind: &str,
    source_url: &str,
    receipt_id: &str,
) -> Result<ar_upload::DataItem, SnapshotError> {
    DataItem::builder(signer)
        .data(data)
        .tags(vec![
            tag("Content-Type", content_type),
            tag("App-Name", APP_NAME),
            tag("Snapshot-Kind", snapshot_kind),
            tag("Source-URL", source_url),
            tag("Receipt-ID", receipt_id),
        ])
        .build()
        .await
        .map_err(|e| SnapshotError::Upload(e.to_string()))
}

fn tag(name: &str, value: &str) -> Tag {
    Tag {
        name: name.to_string(),
        value: value.to_string(),
    }
}

fn private_key_from_env() -> Result<[u8; 32], SnapshotError> {
    let key =
        std::env::var("AR_UPLOAD_ETH_PRIVATE_KEY").map_err(|_| SnapshotError::MissingPrivateKey)?;
    let key = key.trim().strip_prefix("0x").unwrap_or(key.trim());
    if key.len() != 64 {
        return Err(SnapshotError::InvalidPrivateKey(
            "expected 32-byte hex string".to_string(),
        ));
    }

    let mut out = [0u8; 32];
    for (idx, chunk) in key.as_bytes().chunks_exact(2).enumerate() {
        let hex = std::str::from_utf8(chunk)
            .map_err(|e| SnapshotError::InvalidPrivateKey(e.to_string()))?;
        out[idx] = u8::from_str_radix(hex, 16)
            .map_err(|e| SnapshotError::InvalidPrivateKey(e.to_string()))?;
    }
    Ok(out)
}

fn decode_base64_image(value: &str) -> Result<Vec<u8>, SnapshotError> {
    let encoded = value
        .trim()
        .split_once(',')
        .map(|(_, data)| data)
        .unwrap_or_else(|| value.trim());

    STANDARD
        .decode(encoded)
        .or_else(|_| STANDARD_NO_PAD.decode(encoded))
        .or_else(|_| URL_SAFE.decode(encoded))
        .or_else(|_| URL_SAFE_NO_PAD.decode(encoded))
        .map_err(|e| SnapshotError::InvalidScreenshotBase64(e.to_string()))
}

fn find_string_by_keys(value: &Value, keys: &[&str]) -> Option<String> {
    match value {
        Value::Object(map) => {
            for key in keys {
                if let Some(found) = map.get(*key).and_then(value_to_string) {
                    return Some(found);
                }
            }

            map.values()
                .find_map(|nested| find_string_by_keys(nested, keys))
        }
        Value::Array(items) => items
            .iter()
            .find_map(|nested| find_string_by_keys(nested, keys)),
        _ => None,
    }
}

fn find_content_string(value: &Value) -> Option<String> {
    match value {
        Value::Object(map) => {
            if let Some(content) = map.get("content").and_then(value_to_string) {
                return Some(content);
            }

            map.values().find_map(find_content_string)
        }
        Value::Array(items) => items.iter().find_map(find_content_string),
        _ => None,
    }
}

fn value_to_string(value: &Value) -> Option<String> {
    value.as_str().map(ToString::to_string)
}

fn new_receipt_id() -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    format!("snapshot-{millis}")
}
