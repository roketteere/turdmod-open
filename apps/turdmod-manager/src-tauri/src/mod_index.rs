// Talks to the turdmod.com marketplace API. The OAuth agent owns token
// storage; we just accept an optional Bearer token per call.

use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use std::time::Duration;

pub const TURDMOD_API_BASE: &str = "https://turdmod.com";

fn api_base() -> String {
    std::env::var("TURDMOD_API_BASE").unwrap_or_else(|_| TURDMOD_API_BASE.to_string())
}

fn http() -> &'static Client {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        Client::builder()
            .user_agent(concat!("turdmod-manager/", env!("CARGO_PKG_VERSION")))
            .timeout(Duration::from_secs(30))
            .build()
            .expect("reqwest client")
    })
}

// Mirrors the `mods` table row the /api/mods route returns. We omit
// `description` because the listing endpoint includes it but the manager
// only needs it on the detail screen (where ModDetail.mod carries it).
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ModSummary {
    pub id: u64,
    pub slug: String,
    pub name: String,
    pub summary: String,
    #[serde(default)]
    pub tag: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub is_first_party: bool,
    #[serde(default)]
    pub is_published: bool,
    #[serde(default)]
    pub is_premium: bool,
    #[serde(default)]
    pub is_premium_bundled: bool,
    #[serde(default)]
    pub price_cents: Option<u32>,
    #[serde(default)]
    pub download_count: u64,
    #[serde(default)]
    pub rating_avg: Option<String>,
    #[serde(default)]
    pub rating_count: u32,
    #[serde(default)]
    pub published_at: Option<String>,
}

// Raw mod row as returned by the detail endpoint; we keep it as a JSON
// blob so schema drift on the web side doesn't break the desktop client
// for simply rendering the page.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ModDetail {
    pub r#mod: serde_json::Value,
    #[serde(default)]
    pub author: serde_json::Value,
    #[serde(default, rename = "latestVersion")]
    pub latest_version: serde_json::Value,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DownloadInfo {
    pub url: String,
    pub version: String,
    pub sha256: String,
    #[serde(default)]
    pub expires_in: u64,
    #[serde(default)]
    pub size_bytes: Option<u64>,
}

#[derive(Deserialize)]
struct ListResponse {
    mods: Vec<ModSummary>,
}

pub async fn list_mods(
    category: Option<&str>,
    search: Option<&str>,
) -> Result<Vec<ModSummary>> {
    let mut url = format!("{}/api/mods", api_base());
    let mut sep = '?';
    if let Some(c) = category {
        url.push(sep);
        url.push_str(&format!("category={}", urlencode(c)));
        sep = '&';
    }
    if let Some(s) = search {
        url.push(sep);
        url.push_str(&format!("search={}", urlencode(s)));
    }

    let resp = http()
        .get(&url)
        .send()
        .await
        .with_context(|| format!("GET {}", url))?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!("list_mods: {} -> {} {}", url, status, body));
    }
    let parsed: ListResponse = resp.json().await.context("decode /api/mods")?;
    Ok(parsed.mods)
}

pub async fn get_mod(slug: &str) -> Result<ModDetail> {
    let url = format!("{}/api/mods/{}", api_base(), slug);
    let resp = http()
        .get(&url)
        .send()
        .await
        .with_context(|| format!("GET {}", url))?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!("get_mod: {} -> {} {}", url, status, body));
    }
    resp.json().await.context("decode /api/mods/{slug}")
}

pub async fn get_download_url(
    slug: &str,
    auth_token: Option<&str>,
) -> Result<DownloadInfo> {
    let url = format!("{}/api/mods/{}/download", api_base(), slug);
    let mut req = http().get(&url);
    if let Some(token) = auth_token {
        req = req.bearer_auth(token);
    }
    let resp = req
        .send()
        .await
        .with_context(|| format!("GET {}", url))?;
    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(anyhow!("get_download_url: {} -> {} {}", url, status, body));
    }
    resp.json().await.context("decode /api/mods/{slug}/download")
}

// Minimal percent-encoder for query-string values. Avoids pulling in a
// whole crate for two query params.
fn urlencode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        let unreserved = byte.is_ascii_alphanumeric()
            || byte == b'-'
            || byte == b'_'
            || byte == b'.'
            || byte == b'~';
        if unreserved {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{:02X}", byte));
        }
    }
    out
}
