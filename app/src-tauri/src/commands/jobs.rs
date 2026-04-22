use crate::crawler::{self, is_bruntwork_job_url, parse_allowed_job_url, JobDetailsPayload};
use crate::db::{Job, JobFilterOptions, JobQuery};
use crate::AppState;
use tauri::State;

#[tauri::command]
pub(crate) async fn get_jobs(
    state: State<'_, AppState>,
    keyword: Option<String>,
    watchlisted_only: bool,
    days_ago: Option<i64>,
    source: Option<String>,
    job_type: Option<String>,
    pay_range: Option<String>,
    run_id: Option<i64>,
) -> Result<Vec<Job>, String> {
    let mut jobs = state
        .db
        .query_jobs(JobQuery {
            keyword: keyword.as_deref(),
            watchlisted_only,
            days_ago,
            source: source.as_deref(),
            job_type: job_type.as_deref(),
            pay_range: pay_range.as_deref(),
            run_id,
        })
        .map_err(|e| e.to_string())?;

    if let Some(days) = days_ago {
        let now = chrono::Utc::now();
        jobs.retain(
            |job| match crawler::posted_at_days_ago(&job.posted_at, &now) {
                Some(d) => d <= days,
                None => true,
            },
        );
    }

    Ok(jobs)
}

#[tauri::command]
pub(crate) async fn get_job_filter_options(
    state: State<'_, AppState>,
    days_ago: Option<i64>,
) -> Result<JobFilterOptions, String> {
    state
        .db
        .job_filter_options(days_ago)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn get_watchlisted_jobs(state: State<'_, AppState>) -> Result<Vec<Job>, String> {
    state.db.get_watchlisted_jobs().map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn fetch_job_details(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    url: String,
) -> Result<JobDetailsPayload, String> {
    let parsed_url = parse_allowed_job_url(&url)?;
    let mut webview_payload: Option<JobDetailsPayload> = None;
    if is_bruntwork_job_url(&parsed_url) {
        let timeout = std::time::Duration::from_secs(25);
        match crawler::webview_scraper::scrape(&app, &state.webview_scraper, &url, timeout).await {
            Ok(result) => {
                eprintln!(
                    "[webview_scraper] ok for {url} ({} text chars)",
                    result.text_length
                );
                let cleaned_html = crawler::webview_scraper::strip_scripts_and_styles(&result.html);
                match crawler::parse_bruntwork_job_details(&cleaned_html) {
                    Ok(payload)
                        if crawler::is_meaningful_job_details(&payload)
                            && !crawler::is_rsc_garbage(&payload.description)
                            && !crawler::is_rsc_garbage(&payload.description_html) =>
                    {
                        webview_payload = Some(payload);
                    }
                    Ok(_) => {
                        eprintln!(
                            "[webview_scraper] payload not meaningful or RSC-garbage, falling back"
                        );
                    }
                    Err(e) => eprintln!("[webview_scraper] parse failed: {e}"),
                }
            }
            Err(e) => eprintln!("[webview_scraper] failed for {url}: {e}"),
        }
    }

    let payload = match webview_payload {
        Some(p) => p,
        None => state.crawler.fetch_job_details(&url).await?,
    };

    if let Err(e) = state.db.update_job_posted_at(&url, &payload.posted_at) {
        eprintln!("[fetch_job_details] failed to backfill posted_at for {url}: {e}");
    }

    Ok(payload)
}

#[tauri::command]
pub(crate) async fn toggle_watchlist(
    state: State<'_, AppState>,
    job_id: i64,
) -> Result<bool, String> {
    state.db.toggle_watchlist(job_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn toggle_applied(
    state: State<'_, AppState>,
    job_id: i64,
) -> Result<bool, String> {
    state.db.toggle_applied(job_id).map_err(|e| e.to_string())
}
