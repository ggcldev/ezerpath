use crate::ai;
use crate::ai::sentence_service::SentenceServiceClient;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct BackendDiagnostics {
    pub state: String,
    pub ready: bool,
    pub embedding_model: String,
    pub native_embedder_ready: bool,
    pub embeddings_cache_dir: String,
    pub runtime_mode: String,
}

pub fn backend_diagnostics(sentence_service: &SentenceServiceClient) -> BackendDiagnostics {
    let native_embedder_ready = ai::native_embedder::is_ready();
    BackendDiagnostics {
        state: if native_embedder_ready {
            "ready".to_string()
        } else {
            "available".to_string()
        },
        ready: true,
        embedding_model: ai::SUPPORTED_EMBEDDING_MODEL.to_string(),
        native_embedder_ready,
        embeddings_cache_dir: sentence_service.cache_dir().to_string_lossy().to_string(),
        runtime_mode: "native".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{backend_diagnostics, BackendDiagnostics};
    use crate::ai;
    use crate::ai::sentence_service::SentenceServiceClient;
    use tempfile::tempdir;

    #[test]
    fn backend_diagnostics_reports_native_runtime_contract() {
        let tmp = tempdir().expect("failed to create tempdir");
        let cache_dir = tmp.path().join("embeddings_cache");
        let sentence_service = SentenceServiceClient::new(120_000, cache_dir.clone());

        let diagnostics: BackendDiagnostics = backend_diagnostics(&sentence_service);

        assert!(diagnostics.ready);
        assert_eq!(diagnostics.runtime_mode, "native");
        assert_eq!(diagnostics.embedding_model, ai::SUPPORTED_EMBEDDING_MODEL);
        assert_eq!(
            diagnostics.native_embedder_ready,
            ai::native_embedder::is_ready()
        );
        assert_eq!(
            diagnostics.state,
            if diagnostics.native_embedder_ready {
                "ready"
            } else {
                "available"
            }
        );
        assert_eq!(
            diagnostics.embeddings_cache_dir,
            cache_dir.to_string_lossy().to_string()
        );
    }
}
