use crate::ai::ranking::cosine_similarity;
use crate::ai::{
    AiChatFilters, AiChatResponse, AiConversation, AiMessage, EmbeddingIndexStatus,
    KeywordSuggestion, MatchJobResult, ResumeProfileSummary,
};
use crate::crawler::CrawlStats;
use crate::services;
use crate::services::ai_chat_service::{
    begin_chat_turn, classify_intent, handle_describe_intent, handle_followup_intent,
    format_scan_history_reply, handle_general_chat_fallback, handle_ranking_intent,
    handle_search_keyword_intent, intent_name, is_app_scope_query, is_prompt_injection_attempt, out_of_scope_reply,
    persist_blocked_chat_reply, ChatIntent,
};
use crate::AppState;
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;

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

fn import_resume_file(
    app: &tauri::AppHandle,
    file_path: &str,
) -> Result<std::path::PathBuf, String> {
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
    let cfg = state
        .db
        .get_ai_runtime_config()
        .map_err(|e| e.to_string())?;
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
pub(crate) async fn upload_resume(
    state: State<'_, AppState>,
    name: String,
    source_file: Option<String>,
    raw_text: String,
) -> Result<ResumeProfileSummary, String> {
    let normalized_text = raw_text.split_whitespace().collect::<Vec<_>>().join(" ");
    let now = chrono::Utc::now().to_rfc3339();
    let profile = state
        .db
        .save_resume_profile(
            &name,
            source_file.as_deref(),
            &raw_text,
            &normalized_text,
            &now,
        )
        .map_err(|e| e.to_string())?;
    Ok(profile.summary())
}

#[tauri::command]
pub(crate) async fn upload_resume_from_file(
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
    save_imported_resume_from_path(
        &app,
        &state,
        file_path.to_string_lossy().to_string(),
        display_name,
    )
    .await
    .map(Some)
}

#[tauri::command]
pub(crate) async fn list_resumes(
    state: State<'_, AppState>,
) -> Result<Vec<ResumeProfileSummary>, String> {
    state
        .db
        .list_resume_profile_summaries()
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn set_active_resume(
    state: State<'_, AppState>,
    resume_id: i64,
) -> Result<(), String> {
    state
        .db
        .set_active_resume(resume_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn index_jobs_embeddings(
    state: State<'_, AppState>,
) -> Result<EmbeddingIndexStatus, String> {
    let cfg = state
        .db
        .get_ai_runtime_config()
        .map_err(|e| e.to_string())?;
    let embedding_model = cfg.effective_embedding_model();
    let jobs = state
        .db
        .list_jobs_for_embedding()
        .map_err(|e| e.to_string())?;
    if jobs.is_empty() {
        return state
            .db
            .embedding_index_status(embedding_model)
            .map_err(|e| e.to_string());
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

    state
        .db
        .embedding_index_status(embedding_model)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn index_resume_embedding(
    state: State<'_, AppState>,
    resume_id: i64,
) -> Result<EmbeddingIndexStatus, String> {
    let cfg = state
        .db
        .get_ai_runtime_config()
        .map_err(|e| e.to_string())?;
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
    let vector = vectors
        .into_iter()
        .next()
        .ok_or_else(|| "Embedding service returned no vector for resume".to_string())?;
    let now = chrono::Utc::now().to_rfc3339();
    let vector_json = serde_json::to_string(&vector).map_err(|e| e.to_string())?;
    state
        .db
        .upsert_resume_embedding(resume_id, embedding_model, &vector_json, &now)
        .map_err(|e| e.to_string())?;
    state
        .db
        .embedding_index_status(embedding_model)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn embedding_index_status(
    state: State<'_, AppState>,
) -> Result<EmbeddingIndexStatus, String> {
    let cfg = state
        .db
        .get_ai_runtime_config()
        .map_err(|e| e.to_string())?;
    let embedding_model = cfg.effective_embedding_model();
    state
        .db
        .embedding_index_status(embedding_model)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn ai_list_conversations(
    state: State<'_, AppState>,
) -> Result<Vec<AiConversation>, String> {
    state.db.list_ai_conversations().map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn ai_get_conversation(
    state: State<'_, AppState>,
    conversation_id: i64,
) -> Result<Vec<AiMessage>, String> {
    state
        .db
        .get_ai_messages(conversation_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn ai_delete_conversation(
    state: State<'_, AppState>,
    conversation_id: i64,
) -> Result<(), String> {
    state
        .db
        .delete_ai_conversation(conversation_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn ai_clear_conversations(state: State<'_, AppState>) -> Result<(), String> {
    state.db.clear_ai_conversations().map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) async fn ai_chat(
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
        let reply =
            "I can’t follow that request. I only operate within this app’s scope and policy."
                .to_string();
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

    let cfg = state
        .db
        .get_ai_runtime_config()
        .map_err(|e| e.to_string())?;

    let keyword = filters.as_ref().and_then(|f| f.keyword.clone());
    let watchlisted_only = filters
        .as_ref()
        .and_then(|f| f.watchlisted_only)
        .unwrap_or(false);
    let days_ago = filters.as_ref().and_then(|f| f.days_ago);
    let retrieval_start = std::time::Instant::now();
    let jobs = state
        .db
        .get_jobs(keyword.as_deref(), watchlisted_only, days_ago)
        .map_err(|e| e.to_string())?;

    let intent = classify_intent(&message, &recent);
    let intent_str = intent_name(&intent);

    match intent {
        ChatIntent::Ranking { n, ref title_terms } => {
            if let Some(response) = handle_ranking_intent(
                state.db.as_ref(),
                &state.ollama,
                &cfg,
                convo_id,
                &now,
                started,
                retrieval_start,
                intent_str,
                &message,
                &jobs,
                keyword.as_deref(),
                n,
                title_terms,
            )
            .await?
            {
                return Ok(response);
            }
        }
        ChatIntent::FollowUp => {
            if let Some(response) = handle_followup_intent(
                state.db.as_ref(),
                &state.ollama,
                &cfg,
                convo_id,
                &now,
                started,
                retrieval_start,
                intent_str,
                &message,
                &recent,
            )
            .await?
            {
                return Ok(response);
            }
        }
        ChatIntent::Describe { n } => {
            if let Some(response) = handle_describe_intent(
                state.db.as_ref(),
                &state.ollama,
                &cfg,
                convo_id,
                &now,
                started,
                retrieval_start,
                intent_str,
                &message,
                &jobs,
                &recent,
                n,
            )
            .await?
            {
                return Ok(response);
            }
        }
        ChatIntent::SearchKeyword { ref query } => {
            return handle_search_keyword_intent(
                state.db.as_ref(),
                &state.sentence_service,
                &cfg,
                convo_id,
                &now,
                started,
                retrieval_start,
                intent_str,
                query,
            )
            .await;
        }
        ChatIntent::ScanHistory => {
            let runs = state.db.get_runs().map_err(|e| e.to_string())?;
            let reply = format_scan_history_reply(&message, &runs);
            let latency = started.elapsed().as_millis() as i64;
            let _ = state.db.log_ai_run(&crate::db::AiRunLog {
                task_type: "chat",
                latency_ms: latency,
                status: "success_local",
                created_at: &now,
                intent: Some(intent_str),
                route: Some("scan_history_local"),
                ..Default::default()
            });
            state
                .db
                .append_ai_message(
                    convo_id,
                    "assistant",
                    &reply,
                    r#"{"provider":"local","scope":"scan_history"}"#,
                    &[],
                    &now,
                )
                .map_err(|e| e.to_string())?;
            return Ok(AiChatResponse {
                conversation_id: convo_id,
                reply,
                cards: None,
                error: None,
            });
        }
        ChatIntent::General => {}
    }

    handle_general_chat_fallback(
        state.db.as_ref(),
        &state.ollama,
        &cfg,
        convo_id,
        &now,
        started,
        retrieval_start,
        intent_str,
        &jobs,
        &recent,
    )
    .await
}

#[tauri::command]
pub(crate) async fn ai_match_jobs(
    state: State<'_, AppState>,
    resume_id: i64,
    filters: Option<AiChatFilters>,
) -> Result<Vec<MatchJobResult>, String> {
    let cfg = state
        .db
        .get_ai_runtime_config()
        .map_err(|e| e.to_string())?;
    let embedding_model = cfg.effective_embedding_model();
    let resume_vector_json = state
        .db
        .get_resume_embedding(resume_id, embedding_model)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Resume embedding not found. Index resume first.".to_string())?;
    let resume_vector: Vec<f32> =
        serde_json::from_str(&resume_vector_json).map_err(|e| e.to_string())?;

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
                    if row.company.is_empty() {
                        "Unknown company"
                    } else {
                        &row.company
                    },
                    if row.pay.is_empty() { "-" } else { &row.pay },
                    if row.keyword.is_empty() {
                        "-"
                    } else {
                        &row.keyword
                    },
                    row.url
                ),
            })
        })
        .collect();

    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    scored.truncate(20);
    Ok(scored)
}

#[tauri::command]
pub(crate) async fn ai_suggest_keywords(
    _state: State<'_, AppState>,
    _resume_id: i64,
    _current_keywords: Vec<String>,
) -> Result<Vec<KeywordSuggestion>, String> {
    Ok(Vec::new())
}

#[tauri::command]
pub(crate) async fn ai_summarize_job(
    _state: State<'_, AppState>,
    _job_id: i64,
) -> Result<String, String> {
    Ok("Phase A scaffold: job summarization endpoint is wired.".to_string())
}

#[tauri::command]
pub(crate) async fn ai_compare_jobs(
    _state: State<'_, AppState>,
    _job_ids: Vec<i64>,
) -> Result<String, String> {
    Ok("Phase A scaffold: multi-job comparison endpoint is wired.".to_string())
}

#[tauri::command]
pub(crate) async fn ai_start_scan_with_keywords(
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
