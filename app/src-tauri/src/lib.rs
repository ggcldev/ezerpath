pub mod ai;
mod crawler;
pub mod db;
mod services;

use ai::ollama::OllamaClient;
use ai::ollama::ChatMessage;
use ai::prompts::{
    followup_resolution_schema, job_descriptions_response_schema, json_mode_system_suffix,
    top_jobs_response_schema,
};
use ai::ranking::cosine_similarity;
use ai::{
    AiChatError, AiChatFilters, AiChatResponse, AiConversation, AiHealth, AiMessage,
    AiRuntimeConfig, EmbeddingHealth, EmbeddingIndexStatus, KeywordSuggestion, MatchJobResult,
    ResumeProfileSummary,
};
use ai::sentence_service::SentenceServiceClient;
use crawler::{
    is_bruntwork_job_url, parse_allowed_job_url, Crawler, CrawlStats, JobDetailsPayload,
    ScanProgress,
};
use db::{AiRunLog, Database, Job, ScanRun};
use services::ai_chat_service::{
    assistant_meta, assistant_meta_full, begin_chat_turn, build_ollama_system_prompt,
    classify_intent, compact_reply_text, compare_jobs_for_ranking, extract_cards_from_reply,
    format_describe_reply, format_followup_describe_reply, format_followup_select_reply,
    format_ranking_reply, format_search_keyword_reply, get_linked_job_ids, intent_name,
    is_app_scope_query, is_prompt_injection_attempt, job_pay_score_usd_monthly, jobs_to_cards,
    out_of_scope_reply, persist_blocked_chat_reply, response_violates_app_scope,
    scoped_jobs_for_message, semantic_search_fallback, short_description, wants_descriptions,
    ChatIntent, FollowUpResolution, JobDescriptionItem, JobDescriptionsResponse, TopJobsResponse,
    SEARCH_KEYWORD_FTS_MIN_HITS,
};
use std::sync::Arc;
use tauri::ipc::Channel;
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;
use tokio::sync::Mutex;

struct AppState {
    db: Arc<Database>,
    crawler: Crawler,
    ollama: OllamaClient,
    sentence_service: SentenceServiceClient,
    crawl_lock: Mutex<()>,
    webview_scraper: crawler::webview_scraper::WebviewScraperState,
}

#[tauri::command]
async fn crawl_jobs(
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
    let mut jobs = state.db.get_jobs(keyword.as_deref(), watchlisted_only, days_ago).map_err(|e| e.to_string())?;

    // The DB query filters by scraped_at (when we first saw the job).
    // Also filter by posted_at (when it was actually posted on the site)
    // to catch old data that slipped in before the crawler date guard.
    if let Some(days) = days_ago {
        let now = chrono::Utc::now();
        jobs.retain(|job| {
            match crawler::posted_at_days_ago(&job.posted_at, &now) {
                Some(d) => d <= days,
                None => true,
            }
        });
    }

    Ok(jobs)
}

#[tauri::command]
async fn get_watchlisted_jobs(state: State<'_, AppState>) -> Result<Vec<Job>, String> {
    state.db.get_watchlisted_jobs().map_err(|e| e.to_string())
}

#[tauri::command]
async fn fetch_job_details(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    url: String,
) -> Result<JobDetailsPayload, String> {
    let parsed_url = parse_allowed_job_url(&url)?;
    // For JS-rendered sites (currently Bruntwork), try the in-process
    // WebView scraper first. It reuses the WebView Tauri already ships
    // with the app, then falls through to static HTML/RSC parsing if
    // WebView scraping does not produce a meaningful payload.
    let mut webview_payload: Option<JobDetailsPayload> = None;
    if is_bruntwork_job_url(&parsed_url) {
        let timeout = std::time::Duration::from_secs(25);
        match crawler::webview_scraper::scrape(&app, &state.webview_scraper, &url, timeout).await {
            Ok(result) => {
                eprintln!(
                    "[webview_scraper] ok for {url} ({} text chars)",
                    result.text_length
                );
                let cleaned_html =
                    crawler::webview_scraper::strip_scripts_and_styles(&result.html);
                match crawler::parse_bruntwork_job_details(&cleaned_html) {
                    Ok(payload)
                        if crawler::is_meaningful_job_details(&payload)
                            && !crawler::is_rsc_garbage(&payload.description)
                            && !crawler::is_rsc_garbage(&payload.description_html) =>
                    {
                        webview_payload = Some(payload);
                    }
                    Ok(_) => {
                        eprintln!("[webview_scraper] payload not meaningful or RSC-garbage, falling back");
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

    // Backfill the jobs row's posted_at when details fetch finds a date
    // (primarily Bruntwork, whose list page has no reliable date element).
    // Only updates rows that were previously empty.
    if let Err(e) = state.db.update_job_posted_at(&url, &payload.posted_at) {
        eprintln!("[fetch_job_details] failed to backfill posted_at for {url}: {e}");
    }

    Ok(payload)
}

#[tauri::command]
async fn toggle_watchlist(state: State<'_, AppState>, job_id: i64) -> Result<bool, String> {
    state.db.toggle_watchlist(job_id).map_err(|e| e.to_string())
}

#[tauri::command]
async fn toggle_applied(state: State<'_, AppState>, job_id: i64) -> Result<bool, String> {
    state.db.toggle_applied(job_id).map_err(|e| e.to_string())
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

#[tauri::command]
async fn get_ai_runtime_config(state: State<'_, AppState>) -> Result<AiRuntimeConfig, String> {
    state.db.get_ai_runtime_config().map_err(|e| e.to_string())
}

#[tauri::command]
async fn set_ai_runtime_config(state: State<'_, AppState>, config: AiRuntimeConfig) -> Result<(), String> {
    let config = config.validated()?;
    state.db.set_ai_runtime_config(&config).map_err(|e| e.to_string())
}

#[tauri::command]
async fn ai_health_check(state: State<'_, AppState>) -> Result<AiHealth, String> {
    let cfg = state.db.get_ai_runtime_config().map_err(|e| e.to_string())?;
    state.ollama.health_check(&cfg).await
}

#[tauri::command]
async fn ai_list_ollama_models(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    let cfg = state.db.get_ai_runtime_config().map_err(|e| e.to_string())?;
    state.ollama.list_models(&cfg).await
}

#[tauri::command]
async fn ai_embedding_health_check(state: State<'_, AppState>) -> Result<EmbeddingHealth, String> {
    let cfg = state.db.get_ai_runtime_config().map_err(|e| e.to_string())?;
    state.sentence_service.health_check(&cfg).await
}

fn resume_import_dir(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("failed to resolve app data dir: {e}"))?
        .join("resume_imports");
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("failed to create resume import dir: {e}"))?;
    Ok(dir)
}

fn sanitize_resume_file_component(raw: &str) -> String {
    let cleaned = raw
        .chars()
        .map(|ch| match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' => ch,
            _ => '-',
        })
        .collect::<String>();
    let cleaned = cleaned.trim_matches('-');
    if cleaned.is_empty() {
        "resume".to_string()
    } else {
        cleaned.to_string()
    }
}

fn import_resume_file(app: &tauri::AppHandle, file_path: &str) -> Result<std::path::PathBuf, String> {
    let source = std::path::PathBuf::from(file_path);
    let metadata = std::fs::symlink_metadata(&source)
        .map_err(|e| format!("failed to read resume file metadata: {e}"))?;
    if metadata.file_type().is_symlink() {
        return Err("Resume import does not allow symlinks.".to_string());
    }
    if !metadata.is_file() {
        return Err("Resume import expects a regular file.".to_string());
    }

    let ext = source
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .ok_or_else(|| "Unsupported resume file type. Supported: .pdf, .docx, .txt".to_string())?;
    if !matches!(ext.as_str(), "pdf" | "docx" | "txt") {
        return Err("Unsupported resume file type. Supported: .pdf, .docx, .txt".to_string());
    }

    let stem = source
        .file_stem()
        .and_then(|s| s.to_str())
        .map(sanitize_resume_file_component)
        .unwrap_or_else(|| "resume".to_string());
    let imported_name = format!(
        "{}-{}.{}",
        chrono::Utc::now().format("%Y%m%dT%H%M%S%.3fZ"),
        stem,
        ext
    );
    let destination = resume_import_dir(app)?.join(imported_name);
    std::fs::copy(&source, &destination)
        .map_err(|e| format!("failed to import resume file: {e}"))?;
    Ok(destination)
}

async fn save_imported_resume_from_path(
    app: &tauri::AppHandle,
    state: &AppState,
    file_path: String,
    display_name: Option<String>,
) -> Result<ResumeProfileSummary, String> {
    let imported_path = import_resume_file(app, &file_path)?;
    let imported_path_string = imported_path.to_string_lossy().to_string();
    let cfg = state.db.get_ai_runtime_config().map_err(|e| e.to_string())?;
    let extracted = state
        .sentence_service
        .extract_text_from_file(&cfg, imported_path_string.clone())
        .await?;
    let normalized_text = extracted.split_whitespace().collect::<Vec<_>>().join(" ");
    let now = chrono::Utc::now().to_rfc3339();
    let name = display_name.unwrap_or_else(|| {
        std::path::Path::new(&file_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Resume")
            .to_string()
    });
    let profile = state
        .db
        .save_resume_profile(
            &name,
            Some(imported_path_string.as_str()),
            &extracted,
            &normalized_text,
            &now,
        )
        .map_err(|e| e.to_string())?;
    Ok(profile.summary())
}

#[tauri::command]
async fn upload_resume(
    state: State<'_, AppState>,
    name: String,
    source_file: Option<String>,
    raw_text: String,
) -> Result<ResumeProfileSummary, String> {
    let normalized_text = raw_text.split_whitespace().collect::<Vec<_>>().join(" ");
    let now = chrono::Utc::now().to_rfc3339();
    let profile = state
        .db
        .save_resume_profile(&name, source_file.as_deref(), &raw_text, &normalized_text, &now)
        .map_err(|e| e.to_string())?;
    Ok(profile.summary())
}

#[tauri::command]
async fn upload_resume_from_file(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    display_name: Option<String>,
) -> Result<Option<ResumeProfileSummary>, String> {
    let picked = app
        .dialog()
        .file()
        .set_title("Select your resume")
        .add_filter("Resumes", &["pdf", "docx", "txt"])
        .blocking_pick_file();
    let Some(picked) = picked else {
        return Ok(None);
    };
    let file_path = picked
        .into_path()
        .map_err(|_| "failed to resolve the selected resume path".to_string())?;
    save_imported_resume_from_path(&app, &state, file_path.to_string_lossy().to_string(), display_name)
        .await
        .map(Some)
}

#[tauri::command]
async fn list_resumes(state: State<'_, AppState>) -> Result<Vec<ResumeProfileSummary>, String> {
    state.db
        .list_resume_profile_summaries()
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn set_active_resume(state: State<'_, AppState>, resume_id: i64) -> Result<(), String> {
    state.db.set_active_resume(resume_id).map_err(|e| e.to_string())
}

#[tauri::command]
async fn index_jobs_embeddings(state: State<'_, AppState>) -> Result<EmbeddingIndexStatus, String> {
    let cfg = state.db.get_ai_runtime_config().map_err(|e| e.to_string())?;
    // Persist all vectors under the single supported native embedding namespace.
    let embedding_model = cfg.effective_embedding_model();
    let jobs = state.db.list_jobs_for_embedding().map_err(|e| e.to_string())?;
    if jobs.is_empty() {
        return state.db.embedding_index_status(embedding_model).map_err(|e| e.to_string());
    }

    let now = chrono::Utc::now().to_rfc3339();
    let texts = jobs
        .iter()
        .map(|j| {
            format!(
                "Title: {}\nCompany: {}\nPay: {}\nType: {}\nKeyword: {}\nSummary: {}\nURL: {}",
                j.title, j.company, j.pay, j.job_type, j.keyword, j.summary, j.url
            )
        })
        .collect::<Vec<_>>();
    let vectors = state.sentence_service.embed_texts(&cfg, texts).await?;
    if vectors.len() != jobs.len() {
        return Err("Embedding service returned mismatched vector count for jobs".to_string());
    }

    for (job, vector) in jobs.iter().zip(vectors.into_iter()) {
        let vector_json = serde_json::to_string(&vector).map_err(|e| e.to_string())?;
        state
            .db
            .upsert_job_embedding(job.id, embedding_model, &vector_json, &now)
            .map_err(|e| e.to_string())?;
    }

    state.db.embedding_index_status(embedding_model).map_err(|e| e.to_string())
}

#[tauri::command]
async fn index_resume_embedding(state: State<'_, AppState>, resume_id: i64) -> Result<EmbeddingIndexStatus, String> {
    let cfg = state.db.get_ai_runtime_config().map_err(|e| e.to_string())?;
    let embedding_model = cfg.effective_embedding_model();
    let resume = state
        .db
        .get_resume_profile(resume_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Resume not found".to_string())?;
    let text = if resume.normalized_text.trim().is_empty() {
        resume.raw_text
    } else {
        resume.normalized_text
    };
    let vectors = state.sentence_service.embed_texts(&cfg, vec![text]).await?;
    let vector = vectors.into_iter().next().ok_or_else(|| "Embedding service returned no vector for resume".to_string())?;
    let now = chrono::Utc::now().to_rfc3339();
    let vector_json = serde_json::to_string(&vector).map_err(|e| e.to_string())?;
    state
        .db
        .upsert_resume_embedding(resume_id, embedding_model, &vector_json, &now)
        .map_err(|e| e.to_string())?;
    state.db.embedding_index_status(embedding_model).map_err(|e| e.to_string())
}

#[tauri::command]
async fn embedding_index_status(state: State<'_, AppState>) -> Result<EmbeddingIndexStatus, String> {
    let cfg = state.db.get_ai_runtime_config().map_err(|e| e.to_string())?;
    let embedding_model = cfg.effective_embedding_model();
    state
        .db
        .embedding_index_status(embedding_model)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn ai_list_conversations(state: State<'_, AppState>) -> Result<Vec<AiConversation>, String> {
    state.db.list_ai_conversations().map_err(|e| e.to_string())
}

#[tauri::command]
async fn ai_get_conversation(state: State<'_, AppState>, conversation_id: i64) -> Result<Vec<AiMessage>, String> {
    state.db.get_ai_messages(conversation_id).map_err(|e| e.to_string())
}

#[tauri::command]
async fn ai_delete_conversation(state: State<'_, AppState>, conversation_id: i64) -> Result<(), String> {
    state.db.delete_ai_conversation(conversation_id).map_err(|e| e.to_string())
}

#[tauri::command]
async fn ai_clear_conversations(state: State<'_, AppState>) -> Result<(), String> {
    state.db.clear_ai_conversations().map_err(|e| e.to_string())
}

#[tauri::command]
async fn ai_chat(
    state: State<'_, AppState>,
    conversation_id: Option<i64>,
    message: String,
    filters: Option<AiChatFilters>,
) -> Result<AiChatResponse, String> {
    let started = std::time::Instant::now();
    let turn = begin_chat_turn(state.db.as_ref(), conversation_id, &message, 8)?;
    let convo_id = turn.conversation_id;
    let now = turn.now;
    let history = turn.history;
    let recent = turn.recent;

    if is_prompt_injection_attempt(&message) {
        let reply = "I can’t follow that request. I only operate within this app’s scope and policy.".to_string();
        return persist_blocked_chat_reply(
            state.db.as_ref(),
            convo_id,
            &now,
            started,
            reply,
            "blocked_injection",
            "prompt_injection_detected",
            "blocked_injection",
            Some("blocked_injection"),
        );
    }

    if !is_app_scope_query(&message, &history) {
        let reply = out_of_scope_reply();
        return persist_blocked_chat_reply(
            state.db.as_ref(),
            convo_id,
            &now,
            started,
            reply,
            "blocked_scope",
            "out_of_scope_query",
            "blocked_scope",
            Some("blocked"),
        );
    }

    let cfg = state.db.get_ai_runtime_config().map_err(|e| e.to_string())?;

    let keyword = filters.as_ref().and_then(|f| f.keyword.clone());
    let watchlisted_only = filters.as_ref().and_then(|f| f.watchlisted_only).unwrap_or(false);
    let days_ago = filters.as_ref().and_then(|f| f.days_ago);
    let retrieval_start = std::time::Instant::now();
    let jobs = state
        .db
        .get_jobs(keyword.as_deref(), watchlisted_only, days_ago)
        .map_err(|e| e.to_string())?;

    // ── Intent Router ──────────────────────────────────────────────────────
    let intent = classify_intent(&message, &recent);
    let intent_str = intent_name(&intent);

    match intent {
        ChatIntent::Ranking { n, ref title_terms } => {
            // SQL-first: query DB sorted by normalized pay score
            let sql_jobs = state.db
                .get_top_paying_jobs(keyword.as_deref(), title_terms, n)
                .map_err(|e| e.to_string())?;

            // If SQL returned results with salary data, use them directly
            if !sql_jobs.is_empty() {
                let include_desc = wants_descriptions(&message);
                let reply = format_ranking_reply(&sql_jobs, include_desc, true);
                let cards = jobs_to_cards(&sql_jobs);
                let linked_ids: Vec<i64> = cards.iter().map(|c| c.job_id).collect();
                let candidate_ids: Vec<i64> = sql_jobs.iter().filter_map(|j| j.id).collect();
                let retrieval_ms = retrieval_start.elapsed().as_millis() as i64;
                let latency = started.elapsed().as_millis() as i64;
                let _ = state.db.log_ai_run(&AiRunLog {
                    task_type: "chat",
                    latency_ms: latency,
                    status: "success_sql",
                    created_at: &now,
                    intent: Some(intent_str),
                    route: Some("sql_first"),
                    candidate_job_ids: Some(&candidate_ids),
                    final_job_ids: Some(&linked_ids),
                    retrieval_ms: Some(retrieval_ms),
                    ..Default::default()
                });
                state.db.append_ai_message(
                    convo_id, "assistant", &reply,
                    &assistant_meta("sql", None, Some(&cards)),
                    &linked_ids, &now,
                ).map_err(|e| e.to_string())?;
                return Ok(AiChatResponse { conversation_id: convo_id, reply, cards: Some(cards), error: None });
            }

            // Fallback: rank in-memory using normalized pay score, then recency.
            let mut scoped = scoped_jobs_for_message(&message, &jobs, &state.db.get_runs().map_err(|e| e.to_string())?);
            scoped.sort_by(compare_jobs_for_ranking);
            let top: Vec<Job> = scoped.into_iter().take(n).collect();
            if !top.is_empty() {
                let include_desc = wants_descriptions(&message);
                let has_pay_scores = top.iter().any(|j| job_pay_score_usd_monthly(j).is_some());
                let reply = format_ranking_reply(&top, include_desc, has_pay_scores);
                let cards = jobs_to_cards(&top);
                let linked_ids: Vec<i64> = cards.iter().map(|c| c.job_id).collect();
                let candidate_ids: Vec<i64> = top.iter().filter_map(|j| j.id).collect();
                let retrieval_ms = retrieval_start.elapsed().as_millis() as i64;
                let latency = started.elapsed().as_millis() as i64;
                // Ranking ran, but if none of the rows had extractable pay the
                // order is effectively recency, not salary. Flag it so the UI
                // can show a "partial data" badge and the eval can track the
                // degraded-answer rate.
                let degraded_pay = !has_pay_scores;
                let error_code: Option<&str> = if degraded_pay { Some("INSUFFICIENT_DATA") } else { None };
                let chat_error = if degraded_pay {
                    Some(AiChatError {
                        code: "INSUFFICIENT_DATA".to_string(),
                        message: "Ranking by pay not possible — none of the returned jobs had a parseable salary. Ordered by recency instead.".to_string(),
                    })
                } else {
                    None
                };
                let _ = state.db.log_ai_run(&AiRunLog {
                    task_type: "chat",
                    latency_ms: latency,
                    status: if degraded_pay { "partial_local" } else { "success_local" },
                    created_at: &now,
                    intent: Some(intent_str),
                    route: Some(if degraded_pay { "local_ranking_no_pay" } else { "local_ranking" }),
                    candidate_job_ids: Some(&candidate_ids),
                    final_job_ids: Some(&linked_ids),
                    retrieval_ms: Some(retrieval_ms),
                    ..Default::default()
                });
                state.db.append_ai_message(
                    convo_id, "assistant", &reply,
                    &assistant_meta_full("local", None, Some(&cards), error_code),
                    &linked_ids, &now,
                ).map_err(|e| e.to_string())?;
                return Ok(AiChatResponse { conversation_id: convo_id, reply, cards: Some(cards), error: chat_error });
            }

            // JSON-mode ranking: SQL/local found nothing usable. Hand the
            // recency-scoped pool to Ollama and ask it to return a typed
            // TopJobsResponse so we don't have to regex cards out of prose.
            let runs = state.db.get_runs().map_err(|e| e.to_string())?;
            let pool: Vec<Job> = scoped_jobs_for_message(&message, &jobs, &runs)
                .into_iter()
                .take(25)
                .collect();
            if !pool.is_empty() {
                let candidate_ids: Vec<i64> = pool.iter().filter_map(|j| j.id).collect();
                let system = build_ollama_system_prompt(&pool) + &json_mode_system_suffix(&candidate_ids);
                let json_msgs = vec![
                    ChatMessage { role: "system".to_string(), content: system },
                    ChatMessage { role: "user".to_string(), content: message.clone() },
                ];
                let retrieval_ms = retrieval_start.elapsed().as_millis() as i64;
                let llm_start = std::time::Instant::now();
                let json_reply: Result<TopJobsResponse, String> = state
                    .ollama
                    .chat_json(&cfg, json_msgs, top_jobs_response_schema())
                    .await;
                let llm_ms = llm_start.elapsed().as_millis() as i64;
                if let Ok(parsed) = json_reply {
                    let allowed: std::collections::HashSet<i64> = candidate_ids.iter().copied().collect();
                    let valid_ids: Vec<i64> = parsed
                        .jobs
                        .iter()
                        .map(|j| j.job_id)
                        .filter(|id| allowed.contains(id))
                        .take(n)
                        .collect();
                    if !valid_ids.is_empty() {
                        let ranked_jobs = state.db.get_jobs_by_ids(&valid_ids).map_err(|e| e.to_string())?;
                        let include_desc = wants_descriptions(&message);
                        let reply = format_ranking_reply(&ranked_jobs, include_desc, false);
                        let cards = jobs_to_cards(&ranked_jobs);
                        let linked_ids: Vec<i64> = cards.iter().map(|c| c.job_id).collect();
                        let latency = started.elapsed().as_millis() as i64;
                        let _ = state.db.log_ai_run(&AiRunLog {
                            task_type: "chat",
                            latency_ms: latency,
                            status: "success_ollama",
                            created_at: &now,
                            intent: Some(intent_str),
                            route: Some("ollama_ranking_json"),
                            candidate_job_ids: Some(&candidate_ids),
                            final_job_ids: Some(&linked_ids),
                            retrieval_ms: Some(retrieval_ms),
                            llm_ms: Some(llm_ms),
                            ..Default::default()
                        });
                        state.db.append_ai_message(
                            convo_id, "assistant", &reply,
                            &assistant_meta("ollama", None, Some(&cards)),
                            &linked_ids, &now,
                        ).map_err(|e| e.to_string())?;
                        return Ok(AiChatResponse { conversation_id: convo_id, reply, cards: Some(cards), error: None });
                    }
                }
                // JSON parse failed or returned no valid IDs — fall through.
            }
        }

        ChatIntent::FollowUp => {
            // Use linked job IDs from previous assistant message
            let prev_ids = get_linked_job_ids(&recent);
            if prev_ids.is_empty() {
                // No prior linked jobs → the reference words ("those", "each
                // of them", etc.) have nothing to resolve against. Short-circuit
                // with a structured ambiguity error instead of burning an LLM
                // call on a hallucinated answer.
                let retrieval_ms = retrieval_start.elapsed().as_millis() as i64;
                let latency = started.elapsed().as_millis() as i64;
                let reply = "I'm not sure which jobs you're referring to. Try searching or asking for a top list first, then I can describe or compare them."
                    .to_string();
                let _ = state.db.log_ai_run(&AiRunLog {
                    task_type: "chat",
                    latency_ms: latency,
                    status: "ambiguous_reference",
                    created_at: &now,
                    intent: Some(intent_str),
                    route: Some("followup_ambiguous"),
                    retrieval_ms: Some(retrieval_ms),
                    ..Default::default()
                });
                state.db.append_ai_message(
                    convo_id, "assistant", &reply,
                    &assistant_meta_full("local", None, None, Some("AMBIGUOUS_REFERENCE")),
                    &[], &now,
                ).map_err(|e| e.to_string())?;
                return Ok(AiChatResponse {
                    conversation_id: convo_id,
                    reply,
                    cards: None,
                    error: Some(AiChatError {
                        code: "AMBIGUOUS_REFERENCE".to_string(),
                        message: "No prior jobs linked in this conversation.".to_string(),
                    }),
                });
            }
            {
                let linked_jobs = state.db.get_jobs_by_ids(&prev_ids).map_err(|e| e.to_string())?;
                if linked_jobs.is_empty() {
                    // IDs exist in the transcript but the rows are gone (e.g.,
                    // the scan was purged). Report it instead of silently
                    // falling through — the user should know their referents
                    // are no longer retrievable.
                    let retrieval_ms = retrieval_start.elapsed().as_millis() as i64;
                    let latency = started.elapsed().as_millis() as i64;
                    let reply = "The jobs from the earlier message are no longer available — they may have been removed by a later scan.".to_string();
                    let _ = state.db.log_ai_run(&AiRunLog {
                        task_type: "chat",
                        latency_ms: latency,
                        status: "missing_linked_results",
                        created_at: &now,
                        intent: Some(intent_str),
                        route: Some("followup_missing_linked"),
                        candidate_job_ids: Some(&prev_ids),
                        retrieval_ms: Some(retrieval_ms),
                        ..Default::default()
                    });
                    state.db.append_ai_message(
                        convo_id, "assistant", &reply,
                        &assistant_meta_full("local", None, None, Some("MISSING_LINKED_RESULTS")),
                        &[], &now,
                    ).map_err(|e| e.to_string())?;
                    return Ok(AiChatResponse {
                        conversation_id: convo_id,
                        reply,
                        cards: None,
                        error: Some(AiChatError {
                            code: "MISSING_LINKED_RESULTS".to_string(),
                            message: "Linked job rows no longer exist.".to_string(),
                        }),
                    });
                }
                // Fast-path: try the local resolver before touching Ollama.
                // Simple ordinal / prefix-count / select-all phrasing gets
                // answered from prior cards + stored summaries with zero
                // LLM cost and zero parse-failure surface. Anything the
                // resolver isn't confident about falls through to the
                // existing JSON-mode path untouched.
                if let Some(action) = ai::followup::resolve_followup(&message, &prev_ids) {
                    use ai::followup::FollowUpAction;
                    let (selected_ids, wants_description) = match action {
                        FollowUpAction::Select(ids) => (ids, false),
                        FollowUpAction::Describe(ids) => (ids, true),
                    };
                    let selected_set: std::collections::HashSet<i64> =
                        selected_ids.iter().copied().collect();
                    let target_jobs: Vec<Job> = linked_jobs
                        .iter()
                        .filter(|j| j.id.map(|id| selected_set.contains(&id)).unwrap_or(false))
                        .cloned()
                        .collect();
                    if !target_jobs.is_empty() {
                        let reply = if wants_description {
                            format_followup_describe_reply(&target_jobs)
                        } else {
                            format_followup_select_reply(&target_jobs)
                        };
                        let cards = jobs_to_cards(&target_jobs);
                        let linked_ids: Vec<i64> = cards.iter().map(|c| c.job_id).collect();
                        let retrieval_ms = retrieval_start.elapsed().as_millis() as i64;
                        let latency = started.elapsed().as_millis() as i64;
                        let _ = state.db.log_ai_run(&AiRunLog {
                            task_type: "chat",
                            latency_ms: latency,
                            status: "success_followup_local",
                            created_at: &now,
                            intent: Some(intent_str),
                            route: Some(if wants_description {
                                "followup_local_describe"
                            } else {
                                "followup_local_select"
                            }),
                            candidate_job_ids: Some(&prev_ids),
                            final_job_ids: Some(&linked_ids),
                            retrieval_ms: Some(retrieval_ms),
                            ..Default::default()
                        });
                        state.db.append_ai_message(
                            convo_id, "assistant", &reply,
                            &assistant_meta("local", None, Some(&cards)),
                            &linked_ids, &now,
                        ).map_err(|e| e.to_string())?;
                        return Ok(AiChatResponse {
                            conversation_id: convo_id,
                            reply,
                            cards: Some(cards),
                            error: None,
                        });
                    }
                }
                {
                    // Build focused context for Ollama with only the linked jobs
                    let system = build_ollama_system_prompt(&linked_jobs);
                    let mut msgs: Vec<ChatMessage> = vec![ChatMessage { role: "system".to_string(), content: system }];
                    for msg in &recent {
                        if msg.role == "user" || msg.role == "assistant" {
                            msgs.push(ChatMessage { role: msg.role.clone(), content: msg.content.clone() });
                        }
                    }
                    let retrieval_ms = retrieval_start.elapsed().as_millis() as i64;
                    let candidate_ids: Vec<i64> = linked_jobs.iter().filter_map(|j| j.id).collect();
                    // JSON-mode follow-up: ask Ollama to pick which prior job_ids
                    // the user is referring to plus a one-line explanation. We
                    // append a schema instruction to the system message so the
                    // model knows the contract.
                    let mut json_msgs = msgs.clone();
                    if let Some(first) = json_msgs.first_mut() {
                        if first.role == "system" {
                            first.content.push_str(&json_mode_system_suffix(&candidate_ids));
                        }
                    }
                    let llm_start = std::time::Instant::now();
                    let json_reply: Result<FollowUpResolution, String> = state
                        .ollama
                        .chat_json(&cfg, json_msgs, followup_resolution_schema())
                        .await;
                    let llm_ms = llm_start.elapsed().as_millis() as i64;
                    match json_reply {
                        Ok(resolved) => {
                            // Filter target IDs to ones that were actually in
                            // the candidate set — model can hallucinate IDs.
                            let allowed: std::collections::HashSet<i64> = candidate_ids.iter().copied().collect();
                            let target_ids: Vec<i64> = resolved
                                .target_job_ids
                                .iter()
                                .copied()
                                .filter(|id| allowed.contains(id))
                                .collect();
                            let target_jobs = if target_ids.is_empty() {
                                linked_jobs.clone()
                            } else {
                                state.db.get_jobs_by_ids(&target_ids).map_err(|e| e.to_string())?
                            };
                            let mut reply = compact_reply_text(&resolved.explanation);
                            if response_violates_app_scope(&reply) { reply = out_of_scope_reply(); }
                            let cards = jobs_to_cards(&target_jobs);
                            let linked_ids: Vec<i64> = cards.iter().map(|c| c.job_id).collect();
                            let latency = started.elapsed().as_millis() as i64;
                            let _ = state.db.log_ai_run(&AiRunLog {
                                task_type: "chat",
                                latency_ms: latency,
                                status: "success_ollama_followup",
                                created_at: &now,
                                intent: Some(intent_str),
                                route: Some("ollama_followup_json"),
                                candidate_job_ids: Some(&candidate_ids),
                                final_job_ids: Some(&linked_ids),
                                retrieval_ms: Some(retrieval_ms),
                                llm_ms: Some(llm_ms),
                                ..Default::default()
                            });
                            state.db.append_ai_message(
                                convo_id, "assistant", &reply,
                                &assistant_meta("ollama", None, Some(&cards)),
                                &linked_ids, &now,
                            ).map_err(|e| e.to_string())?;
                            return Ok(AiChatResponse { conversation_id: convo_id, reply, cards: Some(cards), error: None });
                        }
                        Err(_) => {} // fall through to general Ollama path
                    }
                }
            }
        }

        ChatIntent::Describe { n } => {
            // First try linked jobs from previous message
            let prev_ids = get_linked_job_ids(&recent);
            let target_jobs = if !prev_ids.is_empty() {
                let linked = state.db.get_jobs_by_ids(&prev_ids).map_err(|e| e.to_string())?;
                if !linked.is_empty() { linked } else { Vec::new() }
            } else {
                Vec::new()
            };

            let target_jobs = if target_jobs.is_empty() {
                // No linked jobs — scope from message
                let runs = state.db.get_runs().map_err(|e| e.to_string())?;
                let scoped = scoped_jobs_for_message(&message, &jobs, &runs);
                scoped.into_iter().take(n).collect::<Vec<_>>()
            } else {
                target_jobs
            };

            if !target_jobs.is_empty() {
                // If any job has a summary, build local reply
                let has_summaries = target_jobs.iter().any(|j| !j.summary.trim().is_empty());
                if has_summaries {
                    let reply = format_describe_reply(&target_jobs);
                    let cards = jobs_to_cards(&target_jobs);
                    let linked_ids: Vec<i64> = cards.iter().map(|c| c.job_id).collect();
                    let candidate_ids: Vec<i64> = target_jobs.iter().filter_map(|j| j.id).collect();
                    let retrieval_ms = retrieval_start.elapsed().as_millis() as i64;
                    let latency = started.elapsed().as_millis() as i64;
                    let _ = state.db.log_ai_run(&AiRunLog {
                        task_type: "chat",
                        latency_ms: latency,
                        status: "success_local",
                        created_at: &now,
                        intent: Some(intent_str),
                        route: Some("local_describe"),
                        candidate_job_ids: Some(&candidate_ids),
                        final_job_ids: Some(&linked_ids),
                        retrieval_ms: Some(retrieval_ms),
                        ..Default::default()
                    });
                    state.db.append_ai_message(
                        convo_id, "assistant", &reply,
                        &assistant_meta("local", None, Some(&cards)),
                        &linked_ids, &now,
                    ).map_err(|e| e.to_string())?;
                    return Ok(AiChatResponse { conversation_id: convo_id, reply, cards: Some(cards), error: None });
                }

                // No summaries — ask Ollama in JSON mode for typed
                // descriptions keyed by job_id, then format locally.
                let candidate_ids: Vec<i64> = target_jobs.iter().filter_map(|j| j.id).collect();
                let system = build_ollama_system_prompt(&target_jobs) + &json_mode_system_suffix(&candidate_ids);
                let json_msgs = vec![
                    ChatMessage { role: "system".to_string(), content: system },
                    ChatMessage { role: "user".to_string(), content: message.clone() },
                ];
                let retrieval_ms = retrieval_start.elapsed().as_millis() as i64;
                let llm_start = std::time::Instant::now();
                let json_reply: Result<JobDescriptionsResponse, String> = state
                    .ollama
                    .chat_json(&cfg, json_msgs, job_descriptions_response_schema())
                    .await;
                let llm_ms = llm_start.elapsed().as_millis() as i64;
                if let Ok(parsed) = json_reply {
                    let allowed: std::collections::HashSet<i64> = candidate_ids.iter().copied().collect();
                    let valid: Vec<JobDescriptionItem> = parsed
                        .jobs
                        .into_iter()
                        .filter(|j| allowed.contains(&j.job_id))
                        .collect();
                    if !valid.is_empty() {
                        let ids: Vec<i64> = valid.iter().map(|v| v.job_id).collect();
                        let lookup = state.db.get_jobs_by_ids(&ids).map_err(|e| e.to_string())?;
                        let mut lines = vec![format!("Descriptions for {} jobs:", valid.len())];
                        for (i, item) in valid.iter().enumerate() {
                            let title = lookup
                                .iter()
                                .find(|j| j.id == Some(item.job_id))
                                .map(|j| j.title.as_str())
                                .unwrap_or("?");
                            lines.push(format!("{}. {}", i + 1, title));
                            lines.push(format!("   {}", short_description(&item.description)));
                        }
                        lines.push("Open any card below for full details.".to_string());
                        let reply = lines.join("\n");
                        let cards = jobs_to_cards(&lookup);
                        let linked_ids: Vec<i64> = cards.iter().map(|c| c.job_id).collect();
                        let latency = started.elapsed().as_millis() as i64;
                        let _ = state.db.log_ai_run(&AiRunLog {
                            task_type: "chat",
                            latency_ms: latency,
                            status: "success_ollama",
                            created_at: &now,
                            intent: Some(intent_str),
                            route: Some("ollama_describe_json"),
                            candidate_job_ids: Some(&candidate_ids),
                            final_job_ids: Some(&linked_ids),
                            retrieval_ms: Some(retrieval_ms),
                            llm_ms: Some(llm_ms),
                            ..Default::default()
                        });
                        state.db.append_ai_message(
                            convo_id, "assistant", &reply,
                            &assistant_meta("ollama", None, Some(&cards)),
                            &linked_ids, &now,
                        ).map_err(|e| e.to_string())?;
                        return Ok(AiChatResponse { conversation_id: convo_id, reply, cards: Some(cards), error: None });
                    }
                }
                // JSON-mode failed or empty — fall through to general Ollama.
            }
        }

        ChatIntent::SearchKeyword { ref query } => {
            let fts_results = state.db.search_jobs_fts(query, 10).map_err(|e| e.to_string())?;
            let fts_ids: std::collections::HashSet<i64> =
                fts_results.iter().filter_map(|j| j.id).collect();

            let (results, route_name) = if fts_results.len() >= SEARCH_KEYWORD_FTS_MIN_HITS {
                (fts_results, "search_keyword_fts")
            } else {
                // Best-effort: if the embedding service is down or no vectors are
                // cached, fall back silently to the FTS hits we already have.
                let want = 10usize.saturating_sub(fts_results.len());
                match semantic_search_fallback(
                    state.db.as_ref(),
                    &state.sentence_service,
                    &cfg,
                    query,
                    &fts_ids,
                    want,
                ).await {
                    Ok(extra) if !extra.is_empty() => {
                        let mut merged = fts_results;
                        merged.extend(extra);
                        (merged, "search_keyword_fts_semantic")
                    }
                    _ => (fts_results, "search_keyword_fts"),
                }
            };

            if !results.is_empty() {
                let reply = format_search_keyword_reply(query, &results);
                let cards = jobs_to_cards(&results);
                let linked_ids: Vec<i64> = cards.iter().map(|c| c.job_id).collect();
                let candidate_ids: Vec<i64> = results.iter().filter_map(|j| j.id).collect();
                let retrieval_ms = retrieval_start.elapsed().as_millis() as i64;
                let latency = started.elapsed().as_millis() as i64;
                let _ = state.db.log_ai_run(&AiRunLog {
                    task_type: "chat",
                    latency_ms: latency,
                    status: "success_search",
                    created_at: &now,
                    intent: Some(intent_str),
                    route: Some(route_name),
                    candidate_job_ids: Some(&candidate_ids),
                    final_job_ids: Some(&linked_ids),
                    retrieval_ms: Some(retrieval_ms),
                    ..Default::default()
                });
                state.db.append_ai_message(
                    convo_id, "assistant", &reply,
                    &assistant_meta("sql", None, Some(&cards)),
                    &linked_ids, &now,
                ).map_err(|e| e.to_string())?;
                return Ok(AiChatResponse { conversation_id: convo_id, reply, cards: Some(cards), error: None });
            }

            // Both FTS and the semantic fallback returned nothing. Short-circuit
            // with a structured soft error instead of handing a vague "I can't
            // find anything" to the general Ollama path — the frontend can
            // render a dedicated empty state and the eval harness can assert
            // on the code rather than prose.
            let retrieval_ms = retrieval_start.elapsed().as_millis() as i64;
            let latency = started.elapsed().as_millis() as i64;
            let reply = format!("No jobs matching \"{query}\" were found in the current scan.");
            let _ = state.db.log_ai_run(&AiRunLog {
                task_type: "chat",
                latency_ms: latency,
                status: "no_matches",
                created_at: &now,
                intent: Some(intent_str),
                route: Some("search_keyword_no_matches"),
                retrieval_ms: Some(retrieval_ms),
                ..Default::default()
            });
            state.db.append_ai_message(
                convo_id, "assistant", &reply,
                &assistant_meta_full("sql", None, None, Some("NO_MATCHES")),
                &[], &now,
            ).map_err(|e| e.to_string())?;
            return Ok(AiChatResponse {
                conversation_id: convo_id,
                reply,
                cards: None,
                error: Some(AiChatError {
                    code: "NO_MATCHES".to_string(),
                    message: format!("No jobs matched '{query}'."),
                }),
            });
        }

        ChatIntent::General => {}
    }

    // ── Ollama Fallback (General path) ─────────────────────────────────────
    let system = build_ollama_system_prompt(&jobs);
    let mut ollama_messages: Vec<ChatMessage> = vec![ChatMessage {
        role: "system".to_string(),
        content: system,
    }];
    for msg in &recent {
        if msg.role == "user" || msg.role == "assistant" || msg.role == "system" {
            ollama_messages.push(ChatMessage {
                role: msg.role.clone(),
                content: msg.content.clone(),
            });
        }
    }

    let candidate_ids: Vec<i64> = jobs.iter().filter_map(|j| j.id).collect();
    let retrieval_ms = retrieval_start.elapsed().as_millis() as i64;
    let llm_start = std::time::Instant::now();
    let ollama_reply = state.ollama.chat(&cfg, ollama_messages).await;
    let llm_ms = llm_start.elapsed().as_millis() as i64;
    let mut reply = match ollama_reply {
        Ok(text) => text,
        Err(err) => {
            let latency = started.elapsed().as_millis() as i64;
            let _ = state.db.log_ai_run(&AiRunLog {
                task_type: "chat",
                latency_ms: latency,
                status: "failed",
                error: Some(&err),
                created_at: &now,
                intent: Some(intent_str),
                route: Some("ollama_streaming"),
                candidate_job_ids: Some(&candidate_ids),
                retrieval_ms: Some(retrieval_ms),
                llm_ms: Some(llm_ms),
                ..Default::default()
            });
            let err_lower = err.to_lowercase();
            let fallback = if err_lower.contains("timed out") || err_lower.contains("error sending request") {
                format!(
                    "Ollama request timed out before completion.\n\
The server is likely reachable, but the response took longer than your timeout.\n\n\
Current timeout: {}ms\n\n\
Quick checks:\n\
1. In Settings > AI Runtime, increase Timeout (ms) to 60000-120000\n\
2. Reduce Max Tokens to 256-512 for faster responses\n\
3. Keep Ollama URL as `{}`\n\
4. Retry your prompt\n\n\
Technical detail: {}",
                    cfg.timeout_ms,
                    cfg.ollama_base_url,
                    err
                )
            } else if err_lower.contains("http 404") || err_lower.contains("model") {
                format!(
                    "Ollama is reachable but the selected model appears unavailable.\n\n\
Selected model: {}\n\n\
Quick checks:\n\
1. Run `ollama list`\n\
2. Pull/select an installed model\n\
3. Retry your prompt\n\n\
Technical detail: {}",
                    cfg.ollama_model,
                    err
                )
            } else {
                format!(
                    "I can’t complete the Ollama request right now.\n\
Please verify local Ollama and retry.\n\n\
Quick checks:\n\
1. Run `ollama serve`\n\
2. Keep Ollama URL as `{}`\n\
3. Ensure your selected model is installed (`ollama list`)\n\
4. Retry your prompt\n\n\
Technical detail: {}",
                    cfg.ollama_base_url,
                    err
                )
            };
            state.db.append_ai_message(
                convo_id, "assistant", &fallback,
                &assistant_meta_full("local", Some("ollama_unreachable"), None, Some("MODEL_ERROR")),
                &[], &now,
            ).map_err(|e| e.to_string())?;
            return Ok(AiChatResponse {
                conversation_id: convo_id,
                reply: fallback,
                cards: None,
                error: Some(AiChatError {
                    code: "MODEL_ERROR".to_string(),
                    message: err,
                }),
            });
        }
    };
    if response_violates_app_scope(&reply) {
        reply = out_of_scope_reply();
    }
    reply = compact_reply_text(&reply);
    let ollama_cards = extract_cards_from_reply(&reply, &jobs);
    let linked_ids: Vec<i64> = ollama_cards.iter().map(|c| c.job_id).collect();
    let latency = started.elapsed().as_millis() as i64;
    let _ = state.db.log_ai_run(&AiRunLog {
        task_type: "chat",
        latency_ms: latency,
        status: "success_ollama",
        created_at: &now,
        intent: Some(intent_str),
        route: Some("ollama_streaming"),
        candidate_job_ids: Some(&candidate_ids),
        final_job_ids: Some(&linked_ids),
        retrieval_ms: Some(retrieval_ms),
        llm_ms: Some(llm_ms),
        ..Default::default()
    });
    state.db.append_ai_message(
        convo_id, "assistant", &reply,
        &assistant_meta("ollama", None, if ollama_cards.is_empty() { None } else { Some(&ollama_cards) }),
        &linked_ids, &now,
    ).map_err(|e| e.to_string())?;

    Ok(AiChatResponse {
        conversation_id: convo_id,
        reply,
        cards: if ollama_cards.is_empty() { None } else { Some(ollama_cards) },
        error: None,
    })
}

#[tauri::command]
async fn ai_match_jobs(
    state: State<'_, AppState>,
    resume_id: i64,
    filters: Option<AiChatFilters>,
) -> Result<Vec<MatchJobResult>, String> {
    let cfg = state.db.get_ai_runtime_config().map_err(|e| e.to_string())?;
    // Matching must read from the same namespace used during indexing.
    let embedding_model = cfg.effective_embedding_model();
    let resume_vector_json = state
        .db
        .get_resume_embedding(resume_id, embedding_model)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Resume embedding not found. Index resume first.".to_string())?;
    let resume_vector: Vec<f32> = serde_json::from_str(&resume_vector_json).map_err(|e| e.to_string())?;

    let mut rows = state
        .db
        .list_job_embeddings(embedding_model)
        .map_err(|e| e.to_string())?;

    if let Some(f) = filters {
        if let Some(keyword) = f.keyword {
            rows.retain(|r| r.keyword.eq_ignore_ascii_case(&keyword));
        }
        if let Some(watchlisted_only) = f.watchlisted_only {
            if watchlisted_only {
                rows.retain(|r| r.watchlisted);
            }
        }
        if let Some(days_ago) = f.days_ago {
            let cutoff = chrono::Utc::now() - chrono::Duration::days(days_ago.max(0));
            rows.retain(|r| {
                chrono::DateTime::parse_from_rfc3339(&r.scraped_at)
                    .ok()
                    .map(|d| d.with_timezone(&chrono::Utc) >= cutoff)
                    .unwrap_or(true)
            });
        }
    }

    let mut scored: Vec<MatchJobResult> = rows
        .into_iter()
        .filter_map(|row| {
            let job_vec: Vec<f32> = serde_json::from_str(&row.vector_json).ok()?;
            let sim = cosine_similarity(&resume_vector, &job_vec);
            let score = (((sim + 1.0) / 2.0) * 100.0).clamp(0.0, 100.0);
            Some(MatchJobResult {
                job_id: row.job_id,
                score,
                reason: format!(
                    "Semantic fit with '{}' at {}. Pay: {}. Keyword: {}. ({})",
                    row.title,
                    if row.company.is_empty() { "Unknown company" } else { &row.company },
                    if row.pay.is_empty() { "-" } else { &row.pay },
                    if row.keyword.is_empty() { "-" } else { &row.keyword },
                    row.url
                ),
            })
        })
        .collect();

    scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(20);
    Ok(scored)
}

#[tauri::command]
async fn ai_suggest_keywords(
    _state: State<'_, AppState>,
    _resume_id: i64,
    _current_keywords: Vec<String>,
) -> Result<Vec<KeywordSuggestion>, String> {
    Ok(Vec::new())
}

#[tauri::command]
async fn ai_summarize_job(_state: State<'_, AppState>, _job_id: i64) -> Result<String, String> {
    Ok("Phase A scaffold: job summarization endpoint is wired.".to_string())
}

#[tauri::command]
async fn ai_compare_jobs(_state: State<'_, AppState>, _job_ids: Vec<i64>) -> Result<String, String> {
    Ok("Phase A scaffold: multi-job comparison endpoint is wired.".to_string())
}

#[tauri::command]
async fn ai_start_scan_with_keywords(
    state: State<'_, AppState>,
    keywords: Vec<String>,
    days: Option<u32>,
) -> Result<Vec<CrawlStats>, String> {
    for kw in &keywords {
        state.db.add_keyword(kw).map_err(|e| e.to_string())?;
    }
    services::scan_service::run_crawl(
        &state.db,
        &state.crawler,
        &state.crawl_lock,
        days,
        None,
        None,
    )
    .await
}

#[tauri::command]
fn backend_diagnostics(state: State<'_, AppState>) -> services::runtime_service::BackendDiagnostics {
    services::runtime_service::backend_diagnostics(&state.sentence_service)
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
