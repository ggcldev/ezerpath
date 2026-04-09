mod crawler;
mod db;

use crawler::{Crawler, CrawlStats};
use db::{Database, Job};
use std::sync::Arc;
use tauri::{Manager, State};

struct AppState {
    db: Arc<Database>,
    crawler: Crawler,
}

#[tauri::command]
async fn crawl_jobs(state: State<'_, AppState>) -> Result<Vec<CrawlStats>, String> {
    let keywords = state.db.get_keywords().map_err(|e| e.to_string())?;
    let mut all_stats = Vec::new();

    for kw in &keywords {
        let stats = state.crawler.crawl_keyword(kw, &state.db).await?;
        all_stats.push(stats);
    }

    Ok(all_stats)
}

#[tauri::command]
async fn get_jobs(state: State<'_, AppState>, keyword: Option<String>, watchlisted_only: bool) -> Result<Vec<Job>, String> {
    state.db.get_jobs(keyword.as_deref(), watchlisted_only).map_err(|e| e.to_string())
}

#[tauri::command]
async fn toggle_watchlist(state: State<'_, AppState>, job_id: i64) -> Result<bool, String> {
    state.db.toggle_watchlist(job_id).map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_keywords(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    state.db.get_keywords().map_err(|e| e.to_string())
}

#[tauri::command]
async fn add_keyword(state: State<'_, AppState>, keyword: String) -> Result<(), String> {
    state.db.add_keyword(&keyword).map_err(|e| e.to_string())
}

#[tauri::command]
async fn remove_keyword(state: State<'_, AppState>, keyword: String) -> Result<(), String> {
    state.db.remove_keyword(&keyword).map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let app_dir = app.path().app_data_dir().expect("failed to get app data dir");
            let db = Arc::new(Database::new(app_dir).expect("failed to init database"));
            let crawler = Crawler::new();

            app.manage(AppState { db, crawler });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            crawl_jobs,
            get_jobs,
            toggle_watchlist,
            get_keywords,
            add_keyword,
            remove_keyword,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
