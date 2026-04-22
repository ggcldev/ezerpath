//! Native in-process sentence embedding using `fastembed` (ONNX Runtime).
//!
//! On first use, the model weights (~90MB for AllMiniLML6V2) are downloaded
//! from HuggingFace and cached under the app data dir. Subsequent uses are
//! offline.
//!
//! The underlying `TextEmbedding` handle is not `Send + Sync` safe across all
//! operations; we wrap it in a `Mutex` and run `embed()` on a blocking thread
//! pool so it plays nicely with Tokio.

use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use tokio::task;

/// Lazy-initialized singleton embedder. Guarded by `OnceLock` so initialization
/// (which may download the model) only happens once across the whole process.
static EMBEDDER: OnceLock<Result<Arc<Mutex<TextEmbedding>>, String>> = OnceLock::new();

fn init_embedder(cache_dir: PathBuf) -> Result<Arc<Mutex<TextEmbedding>>, String> {
    std::fs::create_dir_all(&cache_dir)
        .map_err(|e| format!("cannot create fastembed cache dir: {e}"))?;
    let model = TextEmbedding::try_new(
        InitOptions::new(EmbeddingModel::AllMiniLML6V2)
            .with_cache_dir(cache_dir)
            .with_show_download_progress(false),
    )
    .map_err(|e| format!("fastembed init failed: {e}"))?;
    Ok(Arc::new(Mutex::new(model)))
}

/// Compute embeddings for a batch of texts using the native ONNX model.
///
/// `cache_dir` is where the model weights are stored (typically the app's
/// data dir + "/embeddings_cache").
pub async fn embed_texts(
    cache_dir: PathBuf,
    texts: Vec<String>,
) -> Result<Vec<Vec<f32>>, String> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }

    // Initialize once; reuse on subsequent calls.
    let embedder = EMBEDDER
        .get_or_init(|| init_embedder(cache_dir))
        .as_ref()
        .map_err(|e| e.clone())?
        .clone();

    // fastembed's `embed` is sync and CPU-bound; hand off to blocking pool.
    task::spawn_blocking(move || {
        let guard = embedder
            .lock()
            .map_err(|_| "embedder mutex poisoned".to_string())?;
        let refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
        guard
            .embed(refs, None)
            .map_err(|e| format!("embedding failed: {e}"))
    })
    .await
    .map_err(|e| format!("join error: {e}"))?
}

/// Quick synchronous readiness check — returns true once the singleton has
/// been initialized successfully at least once.
pub fn is_ready() -> bool {
    matches!(EMBEDDER.get(), Some(Ok(_)))
}

/// Force initialization (triggers model download on first call). Safe to call
/// repeatedly — only the first call does work.
pub async fn ensure_initialized(cache_dir: PathBuf) -> Result<(), String> {
    task::spawn_blocking(move || {
        EMBEDDER
            .get_or_init(|| init_embedder(cache_dir))
            .as_ref()
            .map(|_| ())
            .map_err(|e| e.clone())
    })
    .await
    .map_err(|e| format!("join error: {e}"))?
}
