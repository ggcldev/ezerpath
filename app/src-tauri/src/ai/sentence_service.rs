use crate::ai::{EmbeddingHealth, AiRuntimeConfig};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Clone)]
pub struct SentenceServiceClient {
    client: Client,
}

#[derive(Debug, Serialize)]
struct EmbedRequest {
    texts: Vec<String>,
    model: String,
}

#[derive(Debug, Deserialize)]
struct EmbedResponse {
    vectors: Vec<Vec<f32>>,
    model: String,
}

#[derive(Debug, Deserialize)]
struct HealthResponse {
    ok: bool,
    message: String,
}

#[derive(Debug, Serialize)]
struct ExtractTextRequest {
    file_path: String,
}

#[derive(Debug, Deserialize)]
struct ExtractTextResponse {
    text: String,
}

impl SentenceServiceClient {
    pub fn new(timeout_ms: u64) -> Result<Self, reqwest::Error> {
        let client = Client::builder()
            .timeout(Duration::from_millis(timeout_ms))
            .build()?;
        Ok(Self { client })
    }

    pub async fn embed_texts(&self, cfg: &AiRuntimeConfig, texts: Vec<String>) -> Result<Vec<Vec<f32>>, String> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let url = format!("{}/embed", cfg.embedding_service_url.trim_end_matches('/'));
        let req = EmbedRequest {
            texts,
            model: cfg.embedding_model.clone(),
        };
        let resp = self.client.post(url).json(&req).send().await.map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("Embedding service error: HTTP {}", resp.status()));
        }
        let payload: EmbedResponse = resp.json().await.map_err(|e| e.to_string())?;
        if payload.model.is_empty() {
            return Err("Embedding service returned empty model name".to_string());
        }
        Ok(payload.vectors)
    }

    pub async fn health_check(&self, cfg: &AiRuntimeConfig) -> Result<EmbeddingHealth, String> {
        let url = format!("{}/health", cfg.embedding_service_url.trim_end_matches('/'));
        let resp = self.client.get(url).send().await.map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("Embedding health check failed: HTTP {}", resp.status()));
        }
        let payload: HealthResponse = resp.json().await.map_err(|e| e.to_string())?;
        Ok(EmbeddingHealth {
            ok: payload.ok,
            message: payload.message,
            model_name: cfg.embedding_model.clone(),
        })
    }

    pub async fn extract_text_from_file(&self, cfg: &AiRuntimeConfig, file_path: String) -> Result<String, String> {
        let url = format!("{}/extract-text", cfg.embedding_service_url.trim_end_matches('/'));
        let req = ExtractTextRequest { file_path };
        let resp = self.client.post(url).json(&req).send().await.map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("Text extraction failed: HTTP {}", resp.status()));
        }
        let payload: ExtractTextResponse = resp.json().await.map_err(|e| e.to_string())?;
        Ok(payload.text)
    }
}
