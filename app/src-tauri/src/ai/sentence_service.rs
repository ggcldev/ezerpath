use crate::ai::native_embedder;
use crate::ai::native_resume_parser;
use crate::ai::{AiRuntimeConfig, EmbeddingHealth};
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct SentenceServiceClient {
    cache_dir: PathBuf,
}

impl SentenceServiceClient {
    pub fn new(_timeout_ms: u64, cache_dir: PathBuf) -> Self {
        Self { cache_dir }
    }

    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    /// Generate embeddings with the in-process native embedder (fastembed/ONNX).
    pub async fn embed_texts(
        &self,
        _cfg: &AiRuntimeConfig,
        texts: Vec<String>,
    ) -> Result<Vec<Vec<f32>>, String> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        let count = texts.len();
        match native_embedder::embed_texts(self.cache_dir.clone(), texts).await {
            Ok(vecs) => {
                eprintln!("[embedder] native embedded {count} texts");
                Ok(vecs)
            }
            Err(e) => Err(format!("Native embedding failed: {e}")),
        }
    }

    pub async fn health_check(&self, cfg: &AiRuntimeConfig) -> Result<EmbeddingHealth, String> {
        // Native path is healthy if it's either already initialized or initializes on demand.
        // We return OK without actually triggering a download here to keep this call fast.
        if native_embedder::is_ready() {
            return Ok(EmbeddingHealth {
                ok: true,
                message: "Native ONNX embedder ready".to_string(),
                model_name: cfg.effective_embedding_model().to_string(),
            });
        }

        Ok(EmbeddingHealth {
            ok: true,
            message: "Native embedder not yet initialized (will load on first use)".to_string(),
            model_name: cfg.effective_embedding_model().to_string(),
        })
    }

    /// Extract plain text from a resume file (.pdf / .docx / .txt).
    pub async fn extract_text_from_file(
        &self,
        _cfg: &AiRuntimeConfig,
        file_path: String,
    ) -> Result<String, String> {
        let path = PathBuf::from(&file_path);
        match native_resume_parser::extract_text(path).await {
            Ok(text) => {
                eprintln!(
                    "[resume_parser] native extracted {} chars from {}",
                    text.len(),
                    file_path
                );
                Ok(text)
            }
            Err(e) => Err(format!("Native resume text extraction failed: {e}")),
        }
    }
}
