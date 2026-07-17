//! AI Assistant — Tauri command surface for the optional Ollama-powered
//! interpretation layer.
//!
//! Goal (per Joel 2026-05-21): "I want to add so much support that you
//! basically never have to help in that process — an option to have a
//! more powerful mode using Ollama. So I can tick a checkbox or drop
//! to use Ollama and select the model to load or download and use."
//!
//! What this module does:
//! - Detects GPU + VRAM via `nvidia-smi` so the model recommender can
//!   filter to "fits on this machine" (4 GB floor, scales up).
//! - Lists locally installed Ollama models (queries `/api/tags`).
//! - Pulls a new model (streams progress over a Tauri event).
//! - Runs a one-shot chat against the active model.
//! - Higher-level assist helpers that compose `chat()` with structured
//!   prompts — `assist_summarize_diff` is the headline use case (read
//!   `_diff.json` between two builds, return a 5-bullet patch summary).
//!
//! The existing `ollama_pool.rs` handles multi-endpoint dispatch for
//! distributed inference; this module focuses on the LOCAL endpoint
//! the user is selecting in the AI Assistant page. The two coexist
//! cleanly — power users can still hit the pool, casual users tick
//! the box and use whatever local Ollama instance is running.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Runtime};

pub const ASSIST_PROGRESS_EVENT: &str = "assist://progress";

// ---------------------------------------------------------------------------
// GPU detection — used by the front-end to recommend models that fit.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuInfo {
    /// Friendly name reported by nvidia-smi (e.g. "NVIDIA GeForce RTX 5060 Ti").
    pub name: Option<String>,
    /// Total VRAM in MiB. Null if detection failed.
    pub vram_mib: Option<u64>,
    /// Detection source — `"nvidia-smi"` if we got data, `"unavailable"` if not.
    pub source: &'static str,
    /// Free-form note about why detection failed, if applicable.
    pub note: Option<String>,
}

/// Probe nvidia-smi for the primary GPU's name + total VRAM in MiB.
/// Returns `GpuInfo` with `source = "unavailable"` if nvidia-smi isn't
/// on PATH or the query fails — never errors at the Tauri boundary
/// because "no GPU" is a valid UI state (CPU-only Ollama still works
/// for small models, just slower).
#[tauri::command(rename_all = "camelCase")]
pub async fn assist_gpu_info() -> Result<GpuInfo, String> {
    let out = match std::process::Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,memory.total",
            "--format=csv,noheader,nounits",
        ])
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            return Ok(GpuInfo {
                name: None,
                vram_mib: None,
                source: "unavailable",
                note: Some(format!("nvidia-smi not runnable: {}", e)),
            });
        }
    };

    if !out.status.success() {
        return Ok(GpuInfo {
            name: None,
            vram_mib: None,
            source: "unavailable",
            note: Some(String::from_utf8_lossy(&out.stderr).into_owned()),
        });
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    let first_line = stdout.lines().next().unwrap_or("").trim();
    let parts: Vec<&str> = first_line.split(',').map(str::trim).collect();
    if parts.len() < 2 {
        return Ok(GpuInfo {
            name: None,
            vram_mib: None,
            source: "unavailable",
            note: Some(format!("unexpected nvidia-smi output: {}", first_line)),
        });
    }

    let name = parts[0].to_string();
    let vram_mib = parts[1].parse::<u64>().ok();

    Ok(GpuInfo {
        name: if name.is_empty() { None } else { Some(name) },
        vram_mib,
        source: "nvidia-smi",
        note: None,
    })
}

// ---------------------------------------------------------------------------
// Local model registry — list / pull
// ---------------------------------------------------------------------------

const LOCAL_OLLAMA: &str = "http://127.0.0.1:11434";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalModel {
    pub name: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub modified_at: Option<String>,
    /// Hint to the UI: roughly how much VRAM this model needs when
    /// loaded. Derived from `size` as a rough proxy (real headroom
    /// depends on context length + quantization). Good enough for
    /// "fits / tight / doesn't fit" UI decisions.
    #[serde(skip_deserializing)]
    pub estimated_vram_mib: u64,
}

#[derive(Debug, Deserialize)]
struct OllamaTagsResponse {
    #[serde(default)]
    models: Vec<OllamaTagEntry>,
}

#[derive(Debug, Deserialize)]
struct OllamaTagEntry {
    name: String,
    #[serde(default)]
    size: u64,
    #[serde(default)]
    modified_at: Option<String>,
}

/// List models installed in the local Ollama instance. Errors if the
/// local endpoint isn't responding — the UI should surface that as a
/// "start Ollama" hint rather than treating it as a permanent failure.
#[tauri::command(rename_all = "camelCase")]
pub async fn assist_list_models() -> Result<Vec<LocalModel>, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|e| format!("build http client: {}", e))?;
    let resp = client
        .get(format!("{}/api/tags", LOCAL_OLLAMA))
        .send()
        .await
        .map_err(|e| format!("ollama /api/tags: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("ollama returned {}", resp.status()));
    }
    let parsed: OllamaTagsResponse = resp
        .json()
        .await
        .map_err(|e| format!("parse /api/tags: {}", e))?;
    let mut out: Vec<LocalModel> = parsed
        .models
        .into_iter()
        .map(|e| {
            // Rough VRAM estimate: model size on disk plus ~25% for
            // overhead (KV cache, activations). Conservative — real
            // overhead is workload-dependent.
            let est = (e.size as f64 * 1.25 / (1024.0 * 1024.0)) as u64;
            LocalModel {
                name: e.name,
                size: e.size,
                modified_at: e.modified_at,
                estimated_vram_mib: est,
            }
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullProgress {
    pub status: String,
    #[serde(default)]
    pub completed: u64,
    #[serde(default)]
    pub total: u64,
    #[serde(default)]
    pub digest: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PullLine {
    status: String,
    #[serde(default)]
    completed: u64,
    #[serde(default)]
    total: u64,
    #[serde(default)]
    digest: Option<String>,
}

/// Pull a model from the Ollama registry. Streams progress lines to
/// the front-end via `assist://progress` Tauri events. Blocks until
/// the pull is complete (or fails). Returns the final status string.
#[tauri::command(rename_all = "camelCase")]
pub async fn assist_pull_model<R: Runtime>(
    app: AppHandle<R>,
    model: String,
) -> Result<String, String> {
    use futures_util::StreamExt;

    let client = reqwest::Client::builder()
        // Generous timeout — multi-GB pulls can take many minutes.
        .timeout(Duration::from_secs(60 * 60))
        .build()
        .map_err(|e| format!("build http client: {}", e))?;

    let resp = client
        .post(format!("{}/api/pull", LOCAL_OLLAMA))
        .json(&serde_json::json!({ "name": model, "stream": true }))
        .send()
        .await
        .map_err(|e| format!("ollama /api/pull: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("ollama returned {}", resp.status()));
    }

    let mut last_status = String::from("unknown");
    let mut stream = resp.bytes_stream();
    let mut buf: Vec<u8> = Vec::new();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("read chunk: {}", e))?;
        buf.extend_from_slice(&chunk);
        // Ollama streams newline-delimited JSON objects.
        while let Some(pos) = buf.iter().position(|b| *b == b'\n') {
            let line = buf.drain(..=pos).collect::<Vec<u8>>();
            let line_str = String::from_utf8_lossy(&line);
            let line_trim = line_str.trim();
            if line_trim.is_empty() {
                continue;
            }
            if let Ok(parsed) = serde_json::from_str::<PullLine>(line_trim) {
                last_status = parsed.status.clone();
                let _ = app.emit(
                    ASSIST_PROGRESS_EVENT,
                    PullProgress {
                        status: parsed.status,
                        completed: parsed.completed,
                        total: parsed.total,
                        digest: parsed.digest,
                    },
                );
            }
        }
    }
    Ok(last_status)
}

// ---------------------------------------------------------------------------
// Chat — the underlying call all assist helpers funnel into
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct ChatResponse {
    #[serde(default)]
    response: Option<String>,
    #[serde(default)]
    message: Option<ChatMessage>,
}

#[derive(Debug, Deserialize)]
struct ChatMessage {
    #[serde(default)]
    content: String,
}

async fn ollama_chat_inner(
    model: &str,
    prompt: &str,
    system: Option<&str>,
) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .map_err(|e| format!("build http client: {}", e))?;
    // Use /api/generate — simpler than /api/chat for one-shot prompts;
    // returns a single JSON object with `response` rather than streaming
    // a sequence of message tokens.
    let body = serde_json::json!({
        "model": model,
        "prompt": prompt,
        "system": system,
        "stream": false,
    });
    let resp = client
        .post(format!("{}/api/generate", LOCAL_OLLAMA))
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("ollama /api/generate: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("ollama returned {}", resp.status()));
    }
    let parsed: ChatResponse = resp
        .json()
        .await
        .map_err(|e| format!("parse /api/generate: {}", e))?;
    parsed
        .response
        .or_else(|| parsed.message.map(|m| m.content))
        .ok_or_else(|| "ollama returned an empty response".to_string())
}

/// One-shot chat against the local Ollama. The UI uses this as the
/// "Test model" affordance after picking from the dropdown.
#[tauri::command(rename_all = "camelCase")]
pub async fn assist_chat(model: String, prompt: String) -> Result<String, String> {
    ollama_chat_inner(&model, &prompt, None).await
}

// ---------------------------------------------------------------------------
// Higher-level assist helpers — each one composes ollama_chat_inner
// with a structured prompt tuned for the task.
// ---------------------------------------------------------------------------

const DIFF_SUMMARY_SYSTEM: &str = "\
You are an expert game-data analyst summarizing the difference between \
two builds of a Unreal Engine game called SCUM. The user gives you a \
JSON diff with added/removed/changed counts per category (classes, \
enums, structs, SDK headers, widgets, datatables, locres strings). \
Produce a tight 5-bullet summary aimed at a game-modder: what \
changed, what's interesting, what might break their mods. No \
preamble. No fluff. Speculate ONLY about implications you can ground \
in the data. If the diff is empty or all zeros, say so in one line.";

/// Summarize a SCUM dump diff in 5 bullets via the local model. Takes
/// the diff JSON as a string so the front-end can pass either the
/// raw `_diff.json` content or a server-fetched copy.
#[tauri::command(rename_all = "camelCase")]
pub async fn assist_summarize_diff(
    model: String,
    diff_json: String,
) -> Result<String, String> {
    if diff_json.trim().is_empty() {
        return Err("diff_json is empty".to_string());
    }
    let prompt = format!(
        "Here is the diff JSON between two SCUM builds. Summarize in 5 \
         bullets:\n\n```json\n{}\n```",
        diff_json
    );
    ollama_chat_inner(&model, &prompt, Some(DIFF_SUMMARY_SYSTEM)).await
}

const PHASE_LOG_SYSTEM: &str = "\
You are an Unreal Engine modding assistant. The user gives you the \
last N lines of stdout/stderr from a Phase A/B/C extraction run \
against GameServer.exe or SCUM.exe. In 3-5 bullets, tell them: \
(1) what phase did, (2) whether it succeeded or what went wrong, \
(3) the next step. Keep each bullet under 25 words.";

/// Explain a phase log buffer in plain language. Used by the "Explain
/// this" button on the Dump Management log pane.
#[tauri::command(rename_all = "camelCase")]
pub async fn assist_explain_phase_log(
    model: String,
    phase: String,
    log_lines: String,
) -> Result<String, String> {
    if log_lines.trim().is_empty() {
        return Err("log_lines is empty".to_string());
    }
    let prompt = format!(
        "Phase: {}\n\nLog output:\n```\n{}\n```",
        phase, log_lines
    );
    ollama_chat_inner(&model, &prompt, Some(PHASE_LOG_SYSTEM)).await
}
