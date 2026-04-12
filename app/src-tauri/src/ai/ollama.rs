use crate::ai::{AiHealth, AiRuntimeConfig};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::time::timeout;

#[derive(Clone)]
pub struct OllamaClient {
    client: Client,
}

#[derive(Debug, Deserialize)]
struct OllamaTagsResponse {
    models: Vec<OllamaModel>,
}

#[derive(Debug, Deserialize)]
struct OllamaModel {
    name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Serialize)]
struct OllamaChatRequest {
    model: String,
    stream: bool,
    messages: Vec<ChatMessage>,
    options: OllamaChatOptions,
}

#[derive(Debug, Serialize)]
struct OllamaChatOptions {
    temperature: f32,
    num_predict: u32,
}

#[derive(Debug, Deserialize)]
struct OllamaChatResponse {
    message: OllamaMessage,
}

#[derive(Debug, Deserialize)]
struct OllamaMessage {
    content: String,
}

impl OllamaClient {
    pub fn new(timeout_ms: u64) -> Result<Self, reqwest::Error> {
        let client = Client::builder()
            .timeout(Duration::from_millis(timeout_ms))
            .build()?;
        Ok(Self { client })
    }

    pub async fn health_check(&self, cfg: &AiRuntimeConfig) -> Result<AiHealth, String> {
        let base = cfg.ollama_base_url.trim_end_matches('/');
        let url = format!("{base}/api/tags");
        let resp = timeout(Duration::from_millis(cfg.timeout_ms), self.client.get(url).send())
            .await
            .map_err(|_| format!("Ollama health check timed out after {}ms", cfg.timeout_ms))?
            .map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("Ollama health check failed: HTTP {}", resp.status()));
        }

        let payload: OllamaTagsResponse = timeout(Duration::from_millis(cfg.timeout_ms), resp.json())
            .await
            .map_err(|_| format!("Ollama health response parse timed out after {}ms", cfg.timeout_ms))?
            .map_err(|e| e.to_string())?;
        let model_count = payload.models.len();
        let has_selected_model = payload.models.iter().any(|m| m.name == cfg.ollama_model);

        Ok(AiHealth {
            ok: true,
            message: if has_selected_model {
                format!("Ollama reachable. Selected model '{}' is available.", cfg.ollama_model)
            } else {
                format!(
                    "Ollama reachable. Selected model '{}' is not pulled yet.",
                    cfg.ollama_model
                )
            },
            model_count,
        })
    }

    pub async fn list_models(&self, cfg: &AiRuntimeConfig) -> Result<Vec<String>, String> {
        let base = cfg.ollama_base_url.trim_end_matches('/');
        let url = format!("{base}/api/tags");
        let resp = timeout(Duration::from_millis(cfg.timeout_ms), self.client.get(url).send())
            .await
            .map_err(|_| format!("Ollama model list timed out after {}ms", cfg.timeout_ms))?
            .map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("Ollama model list failed: HTTP {}", resp.status()));
        }
        let payload: OllamaTagsResponse = timeout(Duration::from_millis(cfg.timeout_ms), resp.json())
            .await
            .map_err(|_| format!("Ollama model list parse timed out after {}ms", cfg.timeout_ms))?
            .map_err(|e| e.to_string())?;
        let mut names = payload.models.into_iter().map(|m| m.name).collect::<Vec<_>>();
        names.sort();
        names.dedup();
        Ok(names)
    }

    pub async fn chat(&self, cfg: &AiRuntimeConfig, messages: Vec<ChatMessage>) -> Result<String, String> {
        if messages.is_empty() {
            return Err("No messages provided to Ollama chat".to_string());
        }
        let base = cfg.ollama_base_url.trim_end_matches('/');
        let url = format!("{base}/api/chat");
        let req = OllamaChatRequest {
            model: cfg.ollama_model.clone(),
            stream: false,
            messages,
            options: OllamaChatOptions {
                temperature: cfg.temperature,
                num_predict: cfg.max_tokens,
            },
        };
        let resp = timeout(
            Duration::from_millis(cfg.timeout_ms),
            self.client.post(url).json(&req).send(),
        )
        .await
        .map_err(|_| format!("Ollama chat timed out after {}ms", cfg.timeout_ms))?
        .map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("Ollama chat failed: HTTP {}", resp.status()));
        }
        let payload: OllamaChatResponse = timeout(Duration::from_millis(cfg.timeout_ms), resp.json())
            .await
            .map_err(|_| format!("Ollama chat response parse timed out after {}ms", cfg.timeout_ms))?
            .map_err(|e| e.to_string())?;
        Ok(payload.message.content)
    }
}
