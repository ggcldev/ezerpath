mod ai;
mod crawler;
mod db;

use ai::ollama::OllamaClient;
use ai::ollama::ChatMessage;
use ai::prompts::system_prompt_for_job_chat;
use ai::ranking::cosine_similarity;
use ai::{
    AiChatFilters, AiChatResponse, AiConversation, AiHealth, AiJobCard, AiMessage, AiRuntimeConfig,
    EmbeddingHealth, EmbeddingIndexStatus, KeywordSuggestion, MatchJobResult, ResumeProfile,
};
use ai::sentence_service::SentenceServiceClient;
use crawler::{Crawler, CrawlStats, JobDetailsPayload};
use db::{Database, Job, ScanRun};
use std::sync::Arc;
use tauri::{Manager, State};
use tokio::sync::Mutex;

fn extract_top_n(message: &str, default_n: usize) -> usize {
    let lower = message.to_lowercase();
    let tokens = lower
        .split_whitespace()
        .map(|t| t.trim_matches(|c: char| !c.is_ascii_alphanumeric()))
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>();

    let parse_num = |token: &str| -> Option<usize> {
        if let Ok(n) = token.parse::<usize>() {
            if (1..=20).contains(&n) {
                return Some(n);
            }
        }
        None
    };

    // Prefer explicit asks like "top 10", "best 7", or "top10".
    for (i, token) in tokens.iter().enumerate() {
        if *token == "top" || *token == "best" {
            for look_ahead in 1..=3 {
                if let Some(next) = tokens.get(i + look_ahead) {
                    if let Some(n) = parse_num(next) {
                        return n;
                    }
                }
            }
        }
        if token.starts_with("top") {
            let suffix = token.trim_start_matches("top");
            if let Some(n) = parse_num(suffix) {
                return n;
            }
        }
        if token.starts_with("best") {
            let suffix = token.trim_start_matches("best");
            if let Some(n) = parse_num(suffix) {
                return n;
            }
        }
    }

    // Fallback: first number found.
    for token in tokens {
        if let Some(n) = parse_num(token) {
            return n;
        }
    }
    default_n
}

fn chat_title_from_query(message: &str) -> String {
    let normalized = message
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string();
    if normalized.is_empty() {
        return "New Chat".to_string();
    }

    // Keep titles short and readable for sidebar UX.
    const MAX_CHARS: usize = 48;
    if normalized.chars().count() <= MAX_CHARS {
        return normalized;
    }

    let mut out = String::new();
    for ch in normalized.chars().take(MAX_CHARS - 1) {
        out.push(ch);
    }
    out.push('…');
    out
}

fn try_local_top_jobs_reply(message: &str, all_jobs: &[Job], runs: &[ScanRun]) -> Option<(String, Vec<AiJobCard>)> {
    let lower = message.to_lowercase();
    let asks_top = lower.contains("top") || lower.contains("best");
    let asks_jobs = lower.contains("job");
    if !(asks_top && asks_jobs) {
        return None;
    }

    let mut scoped: Vec<Job> = all_jobs.to_vec();
    if lower.contains("latest scan") || lower.contains("last scan") {
        if let Some(latest_run_id) = runs.first().map(|r| r.id) {
            scoped.retain(|j| j.run_id == Some(latest_run_id));
        }
    }
    if scoped.is_empty() {
        return Some((
            "I checked your current app data and there are no jobs in this scope yet.\n\
Try running a new scan or relaxing your filters/keywords.".to_string(),
            Vec::new(),
        ));
    }

    scoped.sort_by(|a, b| b.scraped_at.cmp(&a.scraped_at));
    let n = extract_top_n(message, 3);
    let top = scoped.into_iter().take(n).collect::<Vec<_>>();
    let mut lines = vec![format!("Top {} jobs from your app data:", top.len())];
    let cards = top
        .iter()
        .map(|job| AiJobCard {
            job_id: job.id.unwrap_or_default(),
            title: job.title.clone(),
            company: if job.company.is_empty() {
                "Unknown company".to_string()
            } else {
                job.company.clone()
            },
            pay: if job.pay.is_empty() { "-".to_string() } else { job.pay.clone() },
            posted_at: if job.posted_at.is_empty() {
                "-".to_string()
            } else {
                job.posted_at.clone()
            },
            url: job.url.clone(),
            logo_url: job.company_logo_url.clone(),
        })
        .collect::<Vec<_>>();
    for (i, job) in top.iter().enumerate() {
        lines.push(format!("{}. {}", i + 1, job.title));
    }
    lines.push("Open any card below for full details.".to_string());
    Some((lines.join("\n"), cards))
}

fn out_of_scope_reply() -> String {
    "I’m Ezer, and I’m only made for this app. I can help with scanned jobs, resume matching, keyword suggestions, and job summaries inside Ezerpath.".to_string()
}

fn is_prompt_injection_attempt(message: &str) -> bool {
    let lower = message.to_lowercase();
    let patterns = [
        "ignore previous instructions",
        "ignore all previous instructions",
        "disregard previous instructions",
        "show me your system prompt",
        "reveal system prompt",
        "developer message",
        "jailbreak",
        "bypass",
        "act as a different assistant",
    ];
    patterns.iter().any(|p| lower.contains(p))
}

fn is_app_scope_query(message: &str) -> bool {
    let lower = message.to_lowercase();
    if lower.trim().is_empty() {
        return true;
    }

    // Strong app-domain anchors.
    let app_terms = [
        "job", "jobs", "scan", "keyword", "watchlist", "resume", "profile",
        "match", "summary", "summarize", "posted", "pay", "salary",
        "onlinejobs", "application", "listing", "listings", "ezer",
        "all jobs", "latest scan",
    ];
    // Common clearly out-of-scope intents.
    let outside_terms = [
        "weather", "news", "sports", "bitcoin", "crypto", "stock", "stocks",
        "movie", "music", "recipe", "travel", "translate", "math problem",
        "who is", "president", "prime minister", "celebrity", "wikipedia",
        "youtube", "reddit",
    ];
    if outside_terms.iter().any(|t| lower.contains(t)) {
        return false;
    }

    if app_terms.iter().any(|t| lower.contains(t)) {
        return true;
    }

    // Default conservative behavior: if we don't detect app intent, block.
    false
}

fn response_violates_app_scope(reply: &str) -> bool {
    let lower = reply.to_lowercase();
    let bad_phrases = [
        "as a large language model",
        "as an ai language model",
        "i can't access",
        "i cannot access",
        "i don't have access",
        "i do not have access",
        "i can browse",
        "i can't browse",
        "real-time access",
    ];
    bad_phrases.iter().any(|p| lower.contains(p))
}

fn assistant_meta(provider: &str, scope: Option<&str>, cards: Option<&[AiJobCard]>) -> String {
    let mut meta = serde_json::Map::new();
    meta.insert("provider".to_string(), serde_json::Value::String(provider.to_string()));
    if let Some(scope_val) = scope {
        meta.insert("scope".to_string(), serde_json::Value::String(scope_val.to_string()));
    }
    if let Some(card_rows) = cards {
        if !card_rows.is_empty() {
            meta.insert(
                "cards".to_string(),
                serde_json::to_value(card_rows).unwrap_or(serde_json::Value::Array(vec![])),
            );
        }
    }
    serde_json::Value::Object(meta).to_string()
}

fn compact_reply_text(reply: &str) -> String {
    let mut out_lines: Vec<String> = Vec::new();
    let mut prev_blank = false;
    for raw in reply.lines() {
        let line = raw.trim_end();
        if line.trim().is_empty() {
            if !prev_blank {
                out_lines.push(String::new());
            }
            prev_blank = true;
        } else {
            out_lines.push(line.to_string());
            prev_blank = false;
        }
    }
    let mut compact = out_lines.join("\n").trim().to_string();
    // Hard cap for readability in chat; keep concise unless user asks for depth.
    const MAX_LEN: usize = 1800;
    if compact.len() > MAX_LEN {
        compact.truncate(MAX_LEN);
        compact.push_str("\n\n(Truncated for readability.)");
    }
    compact
}

struct AppState {
    db: Arc<Database>,
    crawler: Crawler,
    ollama: OllamaClient,
    sentence_service: SentenceServiceClient,
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

#[tauri::command]
async fn get_ai_runtime_config(state: State<'_, AppState>) -> Result<AiRuntimeConfig, String> {
    state.db.get_ai_runtime_config().map_err(|e| e.to_string())
}

#[tauri::command]
async fn set_ai_runtime_config(state: State<'_, AppState>, config: AiRuntimeConfig) -> Result<(), String> {
    state.db.set_ai_runtime_config(&config).map_err(|e| e.to_string())
}

#[tauri::command]
async fn ai_health_check(state: State<'_, AppState>) -> Result<AiHealth, String> {
    let cfg = state.db.get_ai_runtime_config().map_err(|e| e.to_string())?;
    state.ollama.health_check(&cfg).await
}

#[tauri::command]
async fn ai_embedding_health_check(state: State<'_, AppState>) -> Result<EmbeddingHealth, String> {
    let cfg = state.db.get_ai_runtime_config().map_err(|e| e.to_string())?;
    state.sentence_service.health_check(&cfg).await
}

#[tauri::command]
async fn upload_resume(
    state: State<'_, AppState>,
    name: String,
    source_file: Option<String>,
    raw_text: String,
) -> Result<ResumeProfile, String> {
    let normalized_text = raw_text.split_whitespace().collect::<Vec<_>>().join(" ");
    let now = chrono::Utc::now().to_rfc3339();
    state
        .db
        .save_resume_profile(&name, source_file.as_deref(), &raw_text, &normalized_text, &now)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn upload_resume_from_file(
    state: State<'_, AppState>,
    file_path: String,
    display_name: Option<String>,
) -> Result<ResumeProfile, String> {
    let cfg = state.db.get_ai_runtime_config().map_err(|e| e.to_string())?;
    let extracted = state
        .sentence_service
        .extract_text_from_file(&cfg, file_path.clone())
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
    state
        .db
        .save_resume_profile(&name, Some(file_path.as_str()), &extracted, &normalized_text, &now)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn list_resumes(state: State<'_, AppState>) -> Result<Vec<ResumeProfile>, String> {
    state.db.list_resume_profiles().map_err(|e| e.to_string())
}

#[tauri::command]
async fn set_active_resume(state: State<'_, AppState>, resume_id: i64) -> Result<(), String> {
    state.db.set_active_resume(resume_id).map_err(|e| e.to_string())
}

#[tauri::command]
async fn index_jobs_embeddings(state: State<'_, AppState>) -> Result<EmbeddingIndexStatus, String> {
    let cfg = state.db.get_ai_runtime_config().map_err(|e| e.to_string())?;
    let jobs = state.db.list_jobs_for_embedding().map_err(|e| e.to_string())?;
    if jobs.is_empty() {
        return state.db.embedding_index_status(&cfg.embedding_model).map_err(|e| e.to_string());
    }

    let now = chrono::Utc::now().to_rfc3339();
    let texts = jobs
        .iter()
        .map(|j| {
            format!(
                "Title: {}\nCompany: {}\nPay: {}\nKeyword: {}\nSummary: {}\nURL: {}",
                j.title, j.company, j.pay, j.keyword, j.summary, j.url
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
            .upsert_job_embedding(job.id, &cfg.embedding_model, &vector_json, &now)
            .map_err(|e| e.to_string())?;
    }

    state.db.embedding_index_status(&cfg.embedding_model).map_err(|e| e.to_string())
}

#[tauri::command]
async fn index_resume_embedding(state: State<'_, AppState>, resume_id: i64) -> Result<EmbeddingIndexStatus, String> {
    let cfg = state.db.get_ai_runtime_config().map_err(|e| e.to_string())?;
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
        .upsert_resume_embedding(resume_id, &cfg.embedding_model, &vector_json, &now)
        .map_err(|e| e.to_string())?;
    state.db.embedding_index_status(&cfg.embedding_model).map_err(|e| e.to_string())
}

#[tauri::command]
async fn embedding_index_status(state: State<'_, AppState>) -> Result<EmbeddingIndexStatus, String> {
    let cfg = state.db.get_ai_runtime_config().map_err(|e| e.to_string())?;
    state
        .db
        .embedding_index_status(&cfg.embedding_model)
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
    let now = chrono::Utc::now().to_rfc3339();
    let suggested_title = chat_title_from_query(&message);
    let convo_id = match conversation_id {
        Some(id) => id,
        None => state
            .db
            .create_ai_conversation(Some(&suggested_title), &now)
            .map_err(|e| e.to_string())?
            .id,
    };

    // Backfill better titles for existing generic conversations.
    state
        .db
        .maybe_set_ai_conversation_title(convo_id, &suggested_title)
        .map_err(|e| e.to_string())?;

    state
        .db
        .append_ai_message(convo_id, "user", &message, "{}", &now)
        .map_err(|e| e.to_string())?;

    if is_prompt_injection_attempt(&message) {
        let reply = "I can’t follow that request. I only operate within this app’s scope and policy.".to_string();
        let latency = started.elapsed().as_millis() as i64;
        let _ = state.db.log_ai_run("chat", latency, "blocked_injection", Some("prompt_injection_detected"), &now);
        state
            .db
            .append_ai_message(
                convo_id,
                "assistant",
                &reply,
                &assistant_meta("local", Some("blocked_injection"), None),
                &now,
            )
            .map_err(|e| e.to_string())?;
        return Ok(AiChatResponse {
            conversation_id: convo_id,
            reply,
            cards: None,
        });
    }

    if !is_app_scope_query(&message) {
        let reply = out_of_scope_reply();
        let latency = started.elapsed().as_millis() as i64;
        let _ = state.db.log_ai_run("chat", latency, "blocked_scope", Some("out_of_scope_query"), &now);
        state
            .db
            .append_ai_message(
                convo_id,
                "assistant",
                &reply,
                &assistant_meta("local", Some("blocked"), None),
                &now,
            )
            .map_err(|e| e.to_string())?;
        return Ok(AiChatResponse {
            conversation_id: convo_id,
            reply,
            cards: None,
        });
    }

    let cfg = state.db.get_ai_runtime_config().map_err(|e| e.to_string())?;
    let history = state.db.get_ai_messages(convo_id).map_err(|e| e.to_string())?;
    let limit_history = 12usize;
    let recent = if history.len() > limit_history {
        history[(history.len() - limit_history)..].to_vec()
    } else {
        history
    };

    let keyword = filters.as_ref().and_then(|f| f.keyword.clone());
    let watchlisted_only = filters.as_ref().and_then(|f| f.watchlisted_only).unwrap_or(false);
    let days_ago = filters.as_ref().and_then(|f| f.days_ago);
    let jobs = state
        .db
        .get_jobs(keyword.as_deref(), watchlisted_only, days_ago)
        .map_err(|e| e.to_string())?;

    let runs = state.db.get_runs().map_err(|e| e.to_string())?;
    if let Some((local_reply, cards)) = try_local_top_jobs_reply(&message, &jobs, &runs) {
        let latency = started.elapsed().as_millis() as i64;
        let _ = state.db.log_ai_run("chat", latency, "success_local", None, &now);
        state
            .db
            .append_ai_message(
                convo_id,
                "assistant",
                &local_reply,
                &assistant_meta("local", None, Some(&cards)),
                &now,
            )
            .map_err(|e| e.to_string())?;
        return Ok(AiChatResponse {
            conversation_id: convo_id,
            reply: local_reply,
            cards: if cards.is_empty() { None } else { Some(cards) },
        });
    }

    let job_context = jobs
        .iter()
        .take(40)
        .map(|j| {
            format!(
                "- [job_id={}] Title: {} | Company: {} | Pay: {} | Keyword: {} | Posted: {} | URL: {} | Summary: {}",
                j.id.unwrap_or_default(),
                j.title,
                if j.company.is_empty() { "-" } else { &j.company },
                if j.pay.is_empty() { "-" } else { &j.pay },
                if j.keyword.is_empty() { "-" } else { &j.keyword },
                if j.posted_at.is_empty() { "-" } else { &j.posted_at },
                if j.url.is_empty() { "-" } else { &j.url },
                if j.summary.is_empty() { "-" } else { &j.summary }
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    let mut ollama_messages: Vec<ChatMessage> = vec![ChatMessage {
        role: "system".to_string(),
        content: format!(
            "{}\n\nRules:\n- Only use local job context below.\n- If the answer is uncertain or missing, say what is missing.\n- Recommend specific next scan keywords when needed.\n- Keep outputs concise, direct, and well-formatted.\n- Use bullets/numbered list for multiple items; avoid long paragraphs.\n- No fluff, no generic disclaimers.\n\nLocal job context ({} rows):\n{}",
            system_prompt_for_job_chat(),
            jobs.len(),
            if job_context.is_empty() { "No jobs available in current filter scope.".to_string() } else { job_context }
        ),
    }];
    for msg in recent {
        if msg.role == "user" || msg.role == "assistant" || msg.role == "system" {
            ollama_messages.push(ChatMessage {
                role: msg.role,
                content: msg.content,
            });
        }
    }

    let ollama_reply = state.ollama.chat(&cfg, ollama_messages).await;
    let mut reply = match ollama_reply {
        Ok(text) => text,
        Err(err) => {
            let latency = started.elapsed().as_millis() as i64;
            let _ = state.db.log_ai_run("chat", latency, "failed", Some(&err), &now);
            return Err(err);
        }
    };
    if response_violates_app_scope(&reply) {
        reply = out_of_scope_reply();
    }
    reply = compact_reply_text(&reply);
    let latency = started.elapsed().as_millis() as i64;
    let _ = state.db.log_ai_run("chat", latency, "success_ollama", None, &now);
    state
        .db
        .append_ai_message(
            convo_id,
            "assistant",
            &reply,
            &assistant_meta("ollama", None, None),
            &now,
        )
        .map_err(|e| e.to_string())?;

    Ok(AiChatResponse {
        conversation_id: convo_id,
        reply,
        cards: None,
    })
}

#[tauri::command]
async fn ai_match_jobs(
    state: State<'_, AppState>,
    resume_id: i64,
    filters: Option<AiChatFilters>,
) -> Result<Vec<MatchJobResult>, String> {
    let cfg = state.db.get_ai_runtime_config().map_err(|e| e.to_string())?;
    let resume_vector_json = state
        .db
        .get_resume_embedding(resume_id, &cfg.embedding_model)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Resume embedding not found. Index resume first.".to_string())?;
    let resume_vector: Vec<f32> = serde_json::from_str(&resume_vector_json).map_err(|e| e.to_string())?;

    let mut rows = state
        .db
        .list_job_embeddings(&cfg.embedding_model)
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
    crawl_jobs(state, days).await
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
            let ollama = OllamaClient::new(30_000)
                .map_err(|e| std::io::Error::other(format!("failed to init ollama client: {e}")))?;
            let sentence_service = SentenceServiceClient::new(30_000)
                .map_err(|e| std::io::Error::other(format!("failed to init sentence service client: {e}")))?;
            app.manage(AppState {
                db,
                crawler,
                ollama,
                sentence_service,
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
            get_ai_runtime_config,
            set_ai_runtime_config,
            ai_health_check,
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
        ])
        .run(tauri::generate_context!());

    if let Err(e) = result {
        eprintln!("error while running tauri application: {e}");
    }
}
