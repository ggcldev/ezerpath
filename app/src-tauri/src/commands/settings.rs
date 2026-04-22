use crate::ai::{AiHealth, AiRuntimeConfig, EmbeddingHealth};
use crate::services;
use crate::AppState;
use tauri::State;

#[tauri::command]
pub(crate) async fn get_ai_runtime_config(
    state: State<'_, AppState>,
) -> Result<AiRuntimeConfig, String> {
    state.db.get_ai_runtime_config().map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn set_ai_runtime_config(
    state: State<'_, AppState>,
    config: AiRuntimeConfig,
) -> Result<(), String> {
    let config = config.validated()?;
    state
        .db
        .set_ai_runtime_config(&config)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn ai_health_check(state: State<'_, AppState>) -> Result<AiHealth, String> {
    let cfg = state
        .db
        .get_ai_runtime_config()
        .map_err(|e| e.to_string())?;
    state.ollama.health_check(&cfg).await
}

#[tauri::command]
pub(crate) async fn ai_list_ollama_models(
    state: State<'_, AppState>,
) -> Result<Vec<String>, String> {
    let cfg = state
        .db
        .get_ai_runtime_config()
        .map_err(|e| e.to_string())?;
    state.ollama.list_models(&cfg).await
}

#[tauri::command]
pub(crate) async fn ai_embedding_health_check(
    state: State<'_, AppState>,
) -> Result<EmbeddingHealth, String> {
    let cfg = state
        .db
        .get_ai_runtime_config()
        .map_err(|e| e.to_string())?;
    state.sentence_service.health_check(&cfg).await
}

#[tauri::command]
pub(crate) fn backend_diagnostics(
    state: State<'_, AppState>,
) -> services::runtime_service::BackendDiagnostics {
    services::runtime_service::backend_diagnostics(&state.sentence_service)
}
