use crate::crawler::{CrawlStats, ScanProgress};
use crate::db::ScanRun;
use crate::services;
use crate::AppState;
use tauri::ipc::Channel;
use tauri::State;

#[tauri::command]
pub(crate) async fn crawl_jobs(
    state: State<'_, AppState>,
    days: Option<u32>,
    sources: Option<Vec<String>>,
    on_progress: Channel<ScanProgress>,
) -> Result<Vec<CrawlStats>, String> {
    services::scan_service::run_crawl(
        &state.db,
        &state.crawler,
        &state.crawl_lock,
        days,
        sources.as_deref(),
        Some(&on_progress),
    )
    .await
}

#[tauri::command]
pub(crate) async fn get_runs(state: State<'_, AppState>) -> Result<Vec<ScanRun>, String> {
    state.db.get_runs().map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn delete_run(state: State<'_, AppState>, run_id: i64) -> Result<(), String> {
    state.db.delete_run(run_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn clear_all_jobs(state: State<'_, AppState>) -> Result<(), String> {
    state.db.clear_all_jobs().map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn get_keywords(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    state.db.get_keywords().map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn add_keyword(state: State<'_, AppState>, keyword: String) -> Result<(), String> {
    state.db.add_keyword(&keyword).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn remove_keyword(
    state: State<'_, AppState>,
    keyword: String,
) -> Result<(), String> {
    state.db.remove_keyword(&keyword).map_err(|e| e.to_string())
}
