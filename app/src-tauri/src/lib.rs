mod crawler;
mod db;

use crawler::{Crawler, CrawlStats, JobDetailsPayload};
use db::{Database, Job, ScanRun};
use std::sync::Arc;
use tauri::{Manager, State};
use tokio::sync::Mutex;

struct AppState {
    db: Arc<Database>,
    crawler: Crawler,
    crawl_lock: Mutex<()>,
}

#[tauri::command]
async fn crawl_jobs(state: State<'_, AppState>, days: Option<u32>) -> Result<Vec<CrawlStats>, String> {
    let _crawl_guard = state
        .crawl_lock
        .try_lock()
        .map_err(|_| "A scan is already in progress".to_string())?;

    let date_days = days.unwrap_or(3);
    let keywords = state.db.get_keywords().map_err(|e| e.to_string())?;

    let started_at = chrono::Utc::now().to_rfc3339();
    let keywords_str = keywords.join(", ");
    let run_id = state.db.insert_run(&keywords_str, &started_at).map_err(|e| e.to_string())?;

    let mut all_stats: Vec<CrawlStats> = Vec::new();
    let mut total_found: i64 = 0;
    let mut total_new: i64 = 0;
    for kw in &keywords {
        match state.crawler.crawl_keyword(kw, &state.db, date_days, run_id).await {
            Ok(stats) => {
                total_found += stats.found as i64;
                total_new += stats.new as i64;
                all_stats.push(stats);
            }
            Err(err) => {
                let finished_at = chrono::Utc::now().to_rfc3339();
                if let Err(mark_err) = state.db.fail_run(run_id, total_found, total_new, &err, &finished_at) {
                    return Err(format!("{err} (failed to mark run failed: {mark_err})"));
                }
                return Err(err);
            }
        }
    }

    let finished_at = chrono::Utc::now().to_rfc3339();
    state.db.complete_run(run_id, total_found, total_new, &finished_at).map_err(|e| e.to_string())?;

    Ok(all_stats)
}

#[tauri::command]
async fn get_runs(state: State<'_, AppState>) -> Result<Vec<ScanRun>, String> {
    state.db.get_runs().map_err(|e| e.to_string())
}

#[tauri::command]
async fn delete_run(state: State<'_, AppState>, run_id: i64) -> Result<(), String> {
    state.db.delete_run(run_id).map_err(|e| e.to_string())
}

#[tauri::command]
async fn clear_all_jobs(state: State<'_, AppState>) -> Result<(), String> {
    state.db.clear_all_jobs().map_err(|e| e.to_string())
}

#[tauri::command]
async fn get_jobs(state: State<'_, AppState>, keyword: Option<String>, watchlisted_only: bool, days_ago: Option<i64>) -> Result<Vec<Job>, String> {
    state.db.get_jobs(keyword.as_deref(), watchlisted_only, days_ago).map_err(|e| e.to_string())
}

#[tauri::command]
async fn fetch_job_details(state: State<'_, AppState>, url: String) -> Result<JobDetailsPayload, String> {
    state.crawler.fetch_job_details(&url).await
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
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let app_dir = app
                .path()
                .app_data_dir()
                .map_err(|e| std::io::Error::other(format!("failed to get app data dir: {e}")))?;
            let db = Arc::new(
                Database::new(app_dir)
                    .map_err(|e| std::io::Error::other(format!("failed to init database: {e}")))?,
            );
            let crawler = Crawler::new()
                .map_err(|e| std::io::Error::other(format!("failed to init crawler: {e}")))?;
            app.manage(AppState {
                db,
                crawler,
                crawl_lock: Mutex::new(()),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            crawl_jobs,
            get_runs,
            delete_run,
            clear_all_jobs,
            get_jobs,
            fetch_job_details,
            toggle_watchlist,
            get_keywords,
            add_keyword,
            remove_keyword,
        ])
        .run(tauri::generate_context!());

    if let Err(e) = result {
        eprintln!("error while running tauri application: {e}");
    }
}
