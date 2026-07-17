// Serial Ollama queue — one concurrent request for CPU-only server

use std::time::Duration;
use tokio::sync::Semaphore;

const MODEL: &str = "scumpilot-fast";
const TIMEOUT: Duration = Duration::from_secs(30);
const FALLBACKS: &[&str] = &["...", "Not now.", "I got nothing to say.", "Leave me alone."];

pub struct OllamaQueue {
    sem: Semaphore,
    idx: std::sync::atomic::AtomicUsize,
}

impl OllamaQueue {
    pub fn new() -> Self {
        Self { sem: Semaphore::new(1), idx: std::sync::atomic::AtomicUsize::new(0) }
    }

    pub async fn generate(&self, system: &str, user: &str) -> String {
        let Ok(_permit) = self.sem.acquire().await else { return self.fallback() };

        let req = crate::ollama::PromptReq {
            model: Some(MODEL.into()),
            prompt: user.to_string(),
            system: Some(system.to_string()),
        };

        match tokio::time::timeout(TIMEOUT, crate::ollama::generate(&req)).await {
            Ok(Ok(resp)) => resp.response.trim().to_string(),
            Ok(Err(e)) => { tracing::warn!("ollama_queue: {}", e); self.fallback() }
            Err(_) => { tracing::warn!("ollama_queue: timeout"); self.fallback() }
        }
    }

    fn fallback(&self) -> String {
        let i = self.idx.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        FALLBACKS[i % FALLBACKS.len()].to_string()
    }
}
