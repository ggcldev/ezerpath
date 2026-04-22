pub mod ai;
mod commands;
mod crawler;
pub mod db;
mod services;

use ai::ollama::OllamaClient;
use ai::sentence_service::SentenceServiceClient;
use commands::ai::*;
use commands::jobs::*;
use commands::scan::*;
use commands::settings::*;
use crawler::Crawler;
use db::Database;
use std::sync::Arc;
use tauri::Manager;
use tokio::sync::Mutex;

pub(crate) struct AppState {
    pub(crate) db: Arc<Database>,
    pub(crate) crawler: Crawler,
    pub(crate) ollama: OllamaClient,
    pub(crate) sentence_service: SentenceServiceClient,
    pub(crate) crawl_lock: Mutex<()>,
    pub(crate) webview_scraper: crawler::webview_scraper::WebviewScraperState,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
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
            let ollama = OllamaClient::new(30_000)
                .map_err(|e| std::io::Error::other(format!("failed to init ollama client: {e}")))?;
            let embeddings_cache_dir = app
                .path()
                .app_data_dir()
                .map(|p| p.join("embeddings_cache"))
                .unwrap_or_else(|_| std::path::PathBuf::from("./embeddings_cache"));
            let sentence_service = SentenceServiceClient::new(30_000, embeddings_cache_dir);
            let webview_scraper = crawler::webview_scraper::WebviewScraperState::new();
            // Register the delivery state separately so the #[tauri::command]
            // scraper_webview_deliver can grab it via `tauri::State`.
            app.manage(webview_scraper.clone());
            app.manage(AppState {
                db,
                crawler,
                ollama,
                sentence_service,
                crawl_lock: Mutex::new(()),
                webview_scraper,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            crawl_jobs,
            get_runs,
            delete_run,
            clear_all_jobs,
            get_jobs,
            get_job_filter_options,
            get_watchlisted_jobs,
            fetch_job_details,
            toggle_watchlist,
            toggle_applied,
            get_keywords,
            add_keyword,
            remove_keyword,
            get_ai_runtime_config,
            set_ai_runtime_config,
            ai_health_check,
            ai_list_ollama_models,
            ai_embedding_health_check,
            upload_resume,
            upload_resume_from_file,
            list_resumes,
            set_active_resume,
            index_jobs_embeddings,
            index_resume_embedding,
            embedding_index_status,
            ai_list_conversations,
            ai_get_conversation,
            ai_delete_conversation,
            ai_clear_conversations,
            ai_chat,
            ai_match_jobs,
            ai_suggest_keywords,
            ai_summarize_job,
            ai_compare_jobs,
            ai_start_scan_with_keywords,
            backend_diagnostics,
            crawler::webview_scraper::scraper_webview_deliver,
        ])
        .run(tauri::generate_context!());

    if let Err(e) = result {
        eprintln!("error while running tauri application: {e}");
    }
}
