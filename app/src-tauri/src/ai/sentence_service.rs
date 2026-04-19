use crate::ai::native_embedder;
use crate::ai::{AiRuntimeConfig, EmbeddingHealth};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Clone)]
pub struct SentenceServiceClient {
    client: Client,
    cache_dir: PathBuf,
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

#[derive(Debug, Serialize)]
struct ExtractTextRequest {
    file_path: String,
}

#[derive(Debug, Deserialize)]
struct ExtractTextResponse {
    text: String,
}

impl SentenceServiceClient {
    pub fn new(timeout_ms: u64, cache_dir: PathBuf) -> Result<Self, reqwest::Error> {
        let client = Client::builder()
            .timeout(Duration::from_millis(timeout_ms))
            .build()?;
        Ok(Self { client, cache_dir })
    }

    /// Generate embeddings. Tries the in-process native embedder first (fastembed/ONNX);
    /// falls back to the Python HTTP service only if native fails. The native path
    /// eliminates the Python dependency and is ~2-3x faster due to no IPC overhead.
    pub async fn embed_texts(
        &self,
        cfg: &AiRuntimeConfig,
        texts: Vec<String>,
    ) -> Result<Vec<Vec<f32>>, String> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        // Try native first
        let count = texts.len();
        match native_embedder::embed_texts(self.cache_dir.clone(), texts.clone()).await {
            Ok(vecs) => {
                eprintln!("[embedder] native embedded {count} texts");
                return Ok(vecs);
            }
            Err(e) => eprintln!("[embedder] native failed ({e}), falling back to HTTP"),
        }

        // Fallback: HTTP service (dev path, kept for now until ai_service/ is deleted)
        let url = format!("{}/embed", cfg.embedding_service_url.trim_end_matches('/'));
        let req = EmbedRequest {
            texts,
            model: cfg.embedding_model.clone(),
        };
        let resp = self
            .client
            .post(url)
            .json(&req)
            .send()
            .await
            .map_err(|e| e.to_string())?;
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
        // Native path is healthy if it's either already initialized or initializes on demand.
        // We return OK without actually triggering a download here to keep this call fast.
        if native_embedder::is_ready() {
            return Ok(EmbeddingHealth {
                ok: true,
                message: "Native ONNX embedder ready".to_string(),
                model_name: "all-MiniLM-L6-v2 (native)".to_string(),
            });
        }

        // Native not initialized yet — report ready anyway (first use will trigger download),
        // but also probe the HTTP fallback as a secondary signal.
        let url = format!("{}/health", cfg.embedding_service_url.trim_end_matches('/'));
        let http_status = self.client.get(url).send().await.ok().map(|r| r.status().is_success());

        Ok(EmbeddingHealth {
            ok: true,
            message: match http_status {
                Some(true) => "Native embedder not yet initialized; HTTP fallback reachable".to_string(),
                _ => "Native embedder not yet initialized (will load on first use)".to_string(),
            },
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
