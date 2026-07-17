// Ollama pool — Tauri commands for the multi-endpoint Ollama dispatcher.
//
// Reads/writes endpoints from %LOCALAPPDATA%/TurdMOD/ollama-endpoints.json
// (auto-materialized from DEFAULT_ENDPOINTS on first start). The shape
// mirrors scripts/ollama-pool/endpoints.json so the CLI scripts and the
// Manager GUI can stay in sync manually until we unify them later.

use std::path::PathBuf;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime};

const ENDPOINTS_FILENAME: &str = "ollama-endpoints.json";

const DEFAULT_ENDPOINTS: &str = r#"{
  "endpoints": [
    {
      "name": "local",
      "url": "http://127.0.0.1:11434",
      "tier": "local",
      "host": "this-box",
      "cost_per_hr": 0,
      "preferred_models": ["qwen2.5-coder:7b", "qwen3.5:9b"],
      "tags": ["fast", "free"]
    },
    {
      "name": "wife-rig",
      "url": "http://YOUR_LAN_IP:11434",
      "tier": "lan",
      "host": "wife-desktop",
      "cost_per_hr": 0,
      "preferred_models": ["qwen2.5-coder:7b"],
      "tags": ["free", "second-brain"]
    }
  ],
  "schema_version": 1
}"#;

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub struct Endpoint {
    pub name: String,
    pub url: String,
    pub tier: String,
    pub host: String,
    #[serde(default)]
    pub cost_per_hr: f64,
    #[serde(default)]
    pub preferred_models: Vec<String>,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[derive(Deserialize, Debug)]
struct EndpointsFile {
    endpoints: Vec<Endpoint>,
    #[serde(default)]
    #[allow(dead_code)]
    schema_version: u32,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct EndpointHealth {
    pub name: String,
    pub url: String,
    pub tier: String,
    pub host: String,
    pub online: bool,
    pub latency_ms: u64,
    pub models: Vec<String>,
    pub model_count: usize,
    pub version: Option<String>,
    pub reason: Option<String>,
    pub cost_per_hr: f64,
    pub tags: Vec<String>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct HealthReport {
    pub checked_at_ms: u128,
    pub endpoints: Vec<EndpointHealth>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DispatchResult {
    pub endpoint: String,
    pub endpoint_url: String,
    pub model: String,
    pub response: Option<String>,
    pub eval_count: Option<u64>,
    pub eval_duration_ms: Option<u64>,
    pub tokens_per_sec: Option<f64>,
    pub total_duration_ms: u64,
    pub prompt_eval_count: Option<u64>,
    pub done_reason: Option<String>,
    pub error: Option<String>,
}

#[derive(Deserialize, Debug)]
struct OllamaTagsResp {
    models: Vec<OllamaTagModel>,
}

#[derive(Deserialize, Debug)]
struct OllamaTagModel {
    name: String,
}

#[derive(Deserialize, Debug)]
struct OllamaVersionResp {
    version: String,
}

#[derive(Deserialize, Debug)]
struct OllamaGenerateResp {
    response: String,
    eval_count: Option<u64>,
    eval_duration: Option<u64>,
    prompt_eval_count: Option<u64>,
    done_reason: Option<String>,
}

fn endpoints_path<R: Runtime>(app: &AppHandle<R>) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_local_data_dir()
        .map_err(|e| format!("could not resolve app data dir: {e}"))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("create app data dir: {e}"))?;
    Ok(dir.join(ENDPOINTS_FILENAME))
}

fn load_endpoints<R: Runtime>(app: &AppHandle<R>) -> Result<Vec<Endpoint>, String> {
    let path = endpoints_path(app)?;
    if !path.exists() {
        std::fs::write(&path, DEFAULT_ENDPOINTS)
            .map_err(|e| format!("write default endpoints: {e}"))?;
    }
    let raw = std::fs::read_to_string(&path).map_err(|e| format!("read endpoints file: {e}"))?;
    let parsed: EndpointsFile =
        serde_json::from_str(&raw).map_err(|e| format!("parse endpoints json: {e}"))?;
    Ok(parsed.endpoints)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn ollama_pool_list_endpoints<R: Runtime>(
    app: AppHandle<R>,
) -> Result<Vec<Endpoint>, String> {
    load_endpoints(&app)
}

#[tauri::command(rename_all = "camelCase")]
pub async fn ollama_pool_endpoints_file_path<R: Runtime>(app: AppHandle<R>) -> Result<String, String> {
    Ok(endpoints_path(&app)?.to_string_lossy().into_owned())
}

#[tauri::command(rename_all = "camelCase")]
pub async fn ollama_pool_health<R: Runtime>(app: AppHandle<R>) -> Result<HealthReport, String> {
    let endpoints = load_endpoints(&app)?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .map_err(|e| format!("build http client: {e}"))?;

    let mut results = Vec::with_capacity(endpoints.len());
    for ep in endpoints {
        let start = Instant::now();
        let mut online = false;
        let mut models: Vec<String> = Vec::new();
        let mut version: Option<String> = None;
        let mut reason: Option<String> = None;

        match client.get(format!("{}/api/tags", ep.url)).send().await {
            Ok(resp) if resp.status().is_success() => {
                match resp.json::<OllamaTagsResp>().await {
                    Ok(tags) => {
                        online = true;
                        models = tags.models.into_iter().map(|m| m.name).collect();
                    }
                    Err(e) => reason = Some(format!("decode tags: {e}")),
                }
            }
            Ok(resp) => reason = Some(format!("HTTP {}", resp.status())),
            Err(e) => reason = Some(e.to_string()),
        }

        if online {
            if let Ok(resp) = client.get(format!("{}/api/version", ep.url)).send().await {
                if let Ok(v) = resp.json::<OllamaVersionResp>().await {
                    version = Some(v.version);
                }
            }
        }

        let latency_ms = start.elapsed().as_millis() as u64;
        let model_count = models.len();

        results.push(EndpointHealth {
            name: ep.name,
            url: ep.url,
            tier: ep.tier,
            host: ep.host,
            online,
            latency_ms,
            models,
            model_count,
            version,
            reason,
            cost_per_hr: ep.cost_per_hr,
            tags: ep.tags,
        });
    }

    let checked_at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);

    Ok(HealthReport {
        checked_at_ms,
        endpoints: results,
    })
}

#[tauri::command(rename_all = "camelCase")]
pub async fn ollama_pool_dispatch<R: Runtime>(
    app: AppHandle<R>,
    endpoint: String,
    prompt: String,
    model: Option<String>,
    num_predict: Option<u32>,
    temperature: Option<f64>,
) -> Result<DispatchResult, String> {
    let endpoints = load_endpoints(&app)?;
    let ep = endpoints
        .iter()
        .find(|e| e.name == endpoint)
        .ok_or_else(|| format!("endpoint '{endpoint}' not found"))?;

    let model_name = model.unwrap_or_else(|| {
        ep.preferred_models
            .first()
            .cloned()
            .unwrap_or_else(|| "qwen2.5-coder:7b".to_string())
    });

    let body = serde_json::json!({
        "model": &model_name,
        "prompt": prompt,
        "stream": false,
        "options": {
            "num_predict": num_predict.unwrap_or(512),
            "temperature": temperature.unwrap_or(0.2),
        }
    });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| format!("build http client: {e}"))?;

    let start = Instant::now();
    let result = client
        .post(format!("{}/api/generate", ep.url))
        .json(&body)
        .send()
        .await;

    let total_ms = start.elapsed().as_millis() as u64;

    match result {
        Ok(resp) if resp.status().is_success() => match resp.json::<OllamaGenerateResp>().await {
            Ok(g) => {
                let eval_duration_ms = g.eval_duration.map(|d| d / 1_000_000);
                let tps = match (g.eval_count, eval_duration_ms) {
                    (Some(c), Some(d)) if d > 0 => Some((c as f64) / ((d as f64) / 1000.0)),
                    _ => None,
                };
                Ok(DispatchResult {
                    endpoint: endpoint.clone(),
                    endpoint_url: ep.url.clone(),
                    model: model_name,
                    response: Some(g.response),
                    eval_count: g.eval_count,
                    eval_duration_ms,
                    tokens_per_sec: tps,
                    total_duration_ms: total_ms,
                    prompt_eval_count: g.prompt_eval_count,
                    done_reason: g.done_reason,
                    error: None,
                })
            }
            Err(e) => Ok(DispatchResult {
                endpoint,
                endpoint_url: ep.url.clone(),
                model: model_name,
                response: None,
                eval_count: None,
                eval_duration_ms: None,
                tokens_per_sec: None,
                total_duration_ms: total_ms,
                prompt_eval_count: None,
                done_reason: None,
                error: Some(format!("decode generate: {e}")),
            }),
        },
        Ok(resp) => Ok(DispatchResult {
            endpoint,
            endpoint_url: ep.url.clone(),
            model: model_name,
            response: None,
            eval_count: None,
            eval_duration_ms: None,
            tokens_per_sec: None,
            total_duration_ms: total_ms,
            prompt_eval_count: None,
            done_reason: None,
            error: Some(format!("HTTP {}", resp.status())),
        }),
        Err(e) => Ok(DispatchResult {
            endpoint,
            endpoint_url: ep.url.clone(),
            model: model_name,
            response: None,
            eval_count: None,
            eval_duration_ms: None,
            tokens_per_sec: None,
            total_duration_ms: total_ms,
            prompt_eval_count: None,
            done_reason: None,
            error: Some(e.to_string()),
        }),
    }
}
