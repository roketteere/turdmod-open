// Ollama proxy — forwards prompts to the local Ollama instance on the server.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const OLLAMA_BASE: &str = "http://localhost:11434";

#[derive(Deserialize)]
pub struct PromptReq {
    pub model: Option<String>,
    pub prompt: String,
    #[serde(default)]
    pub system: Option<String>,
}

#[derive(Serialize)]
pub struct PromptResp {
    pub model: String,
    pub response: String,
}

pub async fn generate(req: &PromptReq) -> Result<PromptResp> {
    let model = req.model.as_deref().unwrap_or("scumpilot-fast");
    let body = serde_json::json!({
        "model": model,
        "prompt": req.prompt,
        "system": req.system,
        "stream": false,
    });

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/api/generate", OLLAMA_BASE))
        .json(&body)
        .timeout(std::time::Duration::from_secs(120))
        .send()
        .await
        .context("ollama request")?;

    let data: serde_json::Value = resp.json().await.context("ollama json")?;
    Ok(PromptResp {
        model: model.to_string(),
        response: data["response"].as_str().unwrap_or("").to_string(),
    })
}

pub async fn list_models() -> Result<serde_json::Value> {
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/api/tags", OLLAMA_BASE))
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .context("ollama tags")?;
    resp.json().await.context("ollama tags json")
}
