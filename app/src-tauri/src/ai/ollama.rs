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
struct OllamaChatStreamChunk {
    #[serde(default)]
    message: Option<OllamaMessage>,
    #[serde(default)]
    done: bool,
    #[serde(default)]
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OllamaMessage {
    #[serde(default)]
    content: String,
}

impl OllamaClient {
    pub fn new(_timeout_ms: u64) -> Result<Self, reqwest::Error> {
        let client = Client::builder()
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
            stream: true,
            messages,
            options: OllamaChatOptions {
                temperature: cfg.temperature,
                num_predict: cfg.max_tokens,
            },
        };

        // The initial connect timeout is bounded by cfg.timeout_ms — Ollama
        // returns headers immediately for streaming requests, so this only
        // catches truly unreachable servers, not slow generations.
        let mut resp = timeout(
            Duration::from_millis(cfg.timeout_ms),
            self.client.post(url).json(&req).send(),
        )
        .await
        .map_err(|_| format!("Ollama chat connect timed out after {}ms", cfg.timeout_ms))?
        .map_err(|e| e.to_string())?;

        if !resp.status().is_success() {
            return Err(format!("Ollama chat failed: HTTP {}", resp.status()));
        }

        // Stream NDJSON chunks. The timeout below is an *idle-gap* budget:
        // we only error if Ollama goes silent for cfg.timeout_ms between
        // tokens. Total generation time is unbounded, so cold model loads
        // and long completions no longer trip a wall-clock limit.
        let idle = Duration::from_millis(cfg.timeout_ms);
        let mut accumulated = String::new();
        let mut buf: Vec<u8> = Vec::new();

        loop {
            let chunk = timeout(idle, resp.chunk())
                .await
                .map_err(|_| format!("Ollama chat idle for {}ms (no tokens received)", cfg.timeout_ms))?
                .map_err(|e| e.to_string())?;

            let Some(bytes) = chunk else { break };
            buf.extend_from_slice(&bytes);

            // Parse complete NDJSON lines from the buffer.
            while let Some(nl) = buf.iter().position(|&b| b == b'\n') {
                let line = buf.drain(..=nl).collect::<Vec<u8>>();
                let line = &line[..line.len() - 1]; // strip \n
                if line.is_empty() {
                    continue;
                }
                let parsed: OllamaChatStreamChunk = serde_json::from_slice(line)
                    .map_err(|e| format!("Ollama chat parse error: {e}"))?;
                if let Some(err) = parsed.error {
                    return Err(format!("Ollama chat error: {err}"));
                }
                if let Some(msg) = parsed.message {
                    accumulated.push_str(&msg.content);
                }
                if parsed.done {
                    return Ok(accumulated);
                }
            }
        }

        // Stream ended without a done=true marker. Try to parse any trailing
        // line in the buffer; otherwise return whatever we accumulated.
        let trailing = buf.iter().position(|&b| b == b'\n').map_or(buf.as_slice(), |i| &buf[..i]);
        if !trailing.is_empty() {
            if let Ok(parsed) = serde_json::from_slice::<OllamaChatStreamChunk>(trailing) {
                if let Some(msg) = parsed.message {
                    accumulated.push_str(&msg.content);
                }
            }
        }
        if accumulated.is_empty() {
            Err("Ollama chat stream ended with no content".to_string())
        } else {
            Ok(accumulated)
        }
    }
}
