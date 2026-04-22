pub mod ai;
mod crawler;
pub mod db;
mod services;

use ai::ollama::OllamaClient;
use ai::ollama::ChatMessage;
use ai::prompts::{
    followup_resolution_schema, job_descriptions_response_schema, json_mode_system_suffix,
    system_prompt_for_job_chat, top_jobs_response_schema,
};
use serde::Deserialize;
use ai::ranking::{cosine_similarity, rank_embeddings_against_query};
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
use db::{parse_pay, AiRunLog, Database, Job, ScanRun};
use services::ai_chat_service::{
    assistant_meta, assistant_meta_full, chat_title_from_query, compact_reply_text,
    extract_cards_from_reply, get_linked_job_ids, is_app_scope_query,
    is_prompt_injection_attempt, jobs_to_cards, out_of_scope_reply, response_violates_app_scope,
    sanitize_text, short_description,
};
use std::cmp::Ordering;
use std::sync::Arc;
use tauri::ipc::Channel;
use tauri::{Manager, State};
use tauri_plugin_dialog::DialogExt;
use tokio::sync::Mutex;

// ── JSON-mode response shapes (phase #4) ───────────────────────────────────
//
// These match the schemas in ai/prompts.rs and are deserialized from
// chat_json output. Kept private — only the routing code in ai_chat reads
// them. Field changes must be mirrored in the schema constants.

#[derive(Debug, Deserialize)]
struct TopJobsResponse {
    #[allow(dead_code)]
    answer_type: String,
    jobs: Vec<TopJobItem>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct TopJobItem {
    // Only `job_id` is consumed downstream — the rest are required by the
    // schema (so the model always emits them) and kept here for forward
    // compatibility / debugging via Debug.
    job_id: i64,
    title: String,
    company: String,
    pay_text: String,
    summary: String,
}

#[derive(Debug, Deserialize)]
struct JobDescriptionsResponse {
    #[allow(dead_code)]
    answer_type: String,
    jobs: Vec<JobDescriptionItem>,
}

#[derive(Debug, Deserialize)]
struct JobDescriptionItem {
    job_id: i64,
    description: String,
}

#[derive(Debug, Deserialize)]
struct FollowUpResolution {
    #[allow(dead_code)]
    answer_type: String,
    target_job_ids: Vec<i64>,
    explanation: String,
}

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

    // No explicit number — if the query uses singular form, return 1.
    let has_plural = lower.contains("jobs")
        || lower.contains("roles")
        || lower.contains("positions")
        || lower.contains("listings")
        || lower.contains("results")
        || lower.contains("options")
        || lower.contains("matches");
    if !has_plural {
        return 1;
    }

    default_n
}

fn wants_descriptions(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.contains("descript")
        || lower.contains("short description")
        || lower.contains("summary")
        || lower.contains("summarize")
        || lower.contains("details")
}

fn is_explicit_top_jobs_request(message: &str) -> bool {
    let lower = message.to_lowercase();
    let has_top_marker = lower.contains("top")
        || lower.contains("best")
        || lower.contains("highest paying")
        || lower.contains("highest-paying");
    if !has_top_marker {
        return false;
    }

    let domain_terms = [
        "job", "jobs", "listing", "listings", "scan result", "scan results",
        "result", "results", "role", "roles", "position", "positions",
        "option", "options", "match", "matches", "opportunit",
    ];
    domain_terms.iter().any(|t| lower.contains(t))
}

fn scoped_jobs_for_message(message: &str, all_jobs: &[Job], runs: &[ScanRun]) -> Vec<Job> {
    let lower = message.to_lowercase();
    let mut scoped: Vec<Job> = all_jobs.to_vec();
    if lower.contains("latest scan") || lower.contains("last scan") {
        if let Some(latest_run_id) = runs.first().map(|r| r.id) {
            scoped.retain(|j| j.run_id == Some(latest_run_id));
        }
    }

    // Filter by job type when the query explicitly requests full-time or part-time roles.
    if lower.contains("full-time") || lower.contains("full time") || lower.contains("fulltime") {
        let typed: Vec<Job> = scoped.iter()
            .filter(|j| j.job_type.to_lowercase().contains("full"))
            .cloned().collect();
        if !typed.is_empty() { scoped = typed; }
    } else if lower.contains("part-time") || lower.contains("part time") || lower.contains("parttime") {
        let typed: Vec<Job> = scoped.iter()
            .filter(|j| j.job_type.to_lowercase().contains("part"))
            .cloned().collect();
        if !typed.is_empty() { scoped = typed; }
    }

    // Filter by meaningful terms from the query that match job titles.
    let stop_words = [
        "can", "you", "provide", "the", "top", "best", "paying", "highest",
        "show", "me", "give", "find", "list", "from", "latest", "last", "scan",
        "job", "jobs", "listing", "listings", "role", "roles", "position", "positions",
        "result", "results", "all", "my", "a", "an", "for", "with", "and", "or",
        "in", "of", "to", "what", "are", "is", "it", "how", "new", "each",
        "option", "options", "recommend", "suggest", "match", "matches",
        "full-time", "part-time", "fulltime", "parttime", "hours", "hour", "weekly",
    ];
    let query_terms: Vec<String> = lower
        .split_whitespace()
        .map(|t| t.trim_matches(|c: char| !c.is_alphanumeric()).to_string())
        .filter(|t| t.len() >= 2 && !stop_words.contains(&t.as_str()))
        .collect();
    if !query_terms.is_empty() {
        let filtered: Vec<Job> = scoped
            .iter()
            .filter(|j| {
                let title_lower = j.title.to_lowercase();
                query_terms.iter().any(|t| title_lower.contains(t.as_str()))
            })
            .cloned()
            .collect();
        // Only apply filter if it doesn't eliminate all results.
        if !filtered.is_empty() {
            scoped = filtered;
        }
    }

    scoped.sort_by(|a, b| b.scraped_at.cmp(&a.scraped_at));
    scoped
}


fn is_follow_up_query(message: &str) -> bool {
    let lower = message.to_lowercase();
    let followups = [
        "which one", "what about", "best one", "top one", "can you", "where", "why",
        "how about", "more details", "description", "summarize", "explain", "compare",
        "that one", "this one",
    ];
    followups.iter().any(|f| lower.contains(f))
}

// ── Intent Router ──────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
enum ChatIntent {
    /// "top 3 paying SEO jobs" → SQL ORDER BY normalized salary score
    Ranking { n: usize, title_terms: Vec<String> },
    /// Follow-up on previous results: "describe them", "which one pays more"
    FollowUp,
    /// "describe the SEO jobs", "summarize the first 3"
    Describe { n: usize },
    /// "find jobs about link building", "show me jobs with seo" → FTS5 search
    SearchKeyword { query: String },
    /// Everything else → Ollama with full job context
    General,
}

fn classify_intent(message: &str, history: &[AiMessage]) -> ChatIntent {
    let lower = message.to_lowercase();

    // Ranking should win over follow-up cues when explicitly requested.
    if is_explicit_top_jobs_request(message) {
        let n = extract_top_n(message, 3);
        let terms = extract_query_terms(&lower);
        return ChatIntent::Ranking { n, title_terms: terms };
    }

    // Keyword search: "find jobs about X", "show me jobs with X", etc.
    // Sits between Ranking and FollowUp so explicit search phrasing wins,
    // but a short reference like "describe them" still routes to follow-up.
    if let Some(query) = try_search_keyword(&lower) {
        return ChatIntent::SearchKeyword { query };
    }

    // Follow-up detection: short message referencing prior results
    let followup_cues = [
        "describe them", "tell me more", "more details", "which one",
        "compare them", "what about", "summarize them", "explain them",
        "short description", "their description", "about these", "about those",
    ];
    let is_followup = followup_cues.iter().any(|c| lower.contains(c))
        || (lower.len() < 60 && (wants_descriptions(&lower) || is_follow_up_query(&lower)));

    // Check if previous assistant message has linked jobs
    let has_linked = history.iter().rev()
        .find(|m| m.role == "assistant")
        .map(|m| {
            serde_json::from_str::<Vec<i64>>(&m.linked_job_ids_json)
                .map(|ids| !ids.is_empty())
                .unwrap_or(false)
        })
        .unwrap_or(false);

    if is_followup && has_linked {
        if wants_descriptions(&lower) {
            let prev_n = history.iter().rev()
                .find(|m| m.role == "assistant")
                .and_then(|m| serde_json::from_str::<Vec<i64>>(&m.linked_job_ids_json).ok())
                .map(|ids| ids.len())
                .unwrap_or(3);
            return ChatIntent::Describe { n: prev_n };
        }
        return ChatIntent::FollowUp;
    }

    // Description request with explicit target
    if wants_descriptions(&lower) && !is_followup {
        let n = extract_top_n(message, 3);
        return ChatIntent::Describe { n };
    }

    ChatIntent::General
}

/// Detect explicit "find/search/show me jobs (about|for|with|...) X" phrasing
/// and return the cleaned search payload. Returns None when no recognizable
/// lead is present or the tail collapses to filler words.
fn try_search_keyword(lower: &str) -> Option<String> {
    // Order matters: longer leads first so "search for jobs" beats "search jobs".
    let leads = [
        "search for jobs", "look for jobs", "find me jobs", "show me jobs",
        "are there jobs", "is there a job", "find jobs", "search jobs", "show jobs",
        "any jobs",
    ];
    let lead_end = leads
        .iter()
        .find_map(|l| lower.find(l).map(|i| i + l.len()))?;
    let tail = lower[lead_end..].trim();
    if tail.is_empty() {
        return None;
    }
    let connectors = [
        "that mention ", "that include ", "related to ", "matching ",
        "involving ", "about ", "with ", "for ", "that ",
    ];
    let after = connectors
        .iter()
        .find_map(|c| tail.strip_prefix(c))
        .unwrap_or(tail);
    let cleaned = after.trim_end_matches(|c: char| !c.is_alphanumeric());
    let filler = ["me", "us", "you", "please", "now", "today", "anyone", "available"];
    let useful: Vec<&str> = cleaned
        .split_whitespace()
        .filter(|w| !filler.contains(w))
        .collect();
    if useful.is_empty() {
        return None;
    }
    Some(useful.join(" "))
}

/// Minimum number of FTS hits before we consider the keyword search "enough".
/// Below this, we fall through to a semantic-similarity pass using cached job
/// embeddings to pick up conceptually-related postings the tokenizer missed.
const SEARCH_KEYWORD_FTS_MIN_HITS: usize = 3;

/// Discard semantic matches whose cosine similarity falls below this floor.
/// Cosine on the sentence-transformer models we use produces roughly [-0.1, 1.0]
/// for job text; 0.30 empirically separates "related" from "unrelated noise".
const SEMANTIC_FALLBACK_SIM_FLOOR: f32 = 0.30;

async fn semantic_search_fallback(
    state: &AppState,
    cfg: &AiRuntimeConfig,
    query: &str,
    exclude: &std::collections::HashSet<i64>,
    limit: usize,
) -> Result<Vec<Job>, String> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    // Native embeddings currently support one canonical model namespace only.
    let embedding_model = cfg.effective_embedding_model();
    let rows = state
        .db
        .list_job_embeddings(embedding_model)
        .map_err(|e| e.to_string())?;
    if rows.is_empty() {
        return Ok(Vec::new());
    }
    let mut vectors = state
        .sentence_service
        .embed_texts(cfg, vec![query.to_string()])
        .await?;
    let query_vec = vectors.pop().ok_or_else(|| "empty query embedding".to_string())?;

    let candidates = rows.into_iter().filter_map(|row| {
        let job_vec: Vec<f32> = serde_json::from_str(&row.vector_json).ok()?;
        Some((row.job_id, job_vec))
    });
    let ids = rank_embeddings_against_query(
        &query_vec,
        candidates,
        exclude,
        SEMANTIC_FALLBACK_SIM_FLOOR,
        limit,
    );
    state.db.get_jobs_by_ids(&ids).map_err(|e| e.to_string())
}

fn format_followup_select_reply(jobs: &[Job]) -> String {
    if jobs.is_empty() {
        return "No matching jobs from the previous result.".to_string();
    }
    let lead = match jobs.len() {
        1 => "Here's the one you picked:".to_string(),
        n => format!("Here are the {n} you picked:"),
    };
    let mut lines = vec![lead];
    for (i, j) in jobs.iter().enumerate() {
        let company = if j.company.is_empty() { "Unknown company" } else { j.company.as_str() };
        lines.push(format!("{}. {} — {}", i + 1, j.title, company));
    }
    lines.join("\n")
}

fn format_followup_describe_reply(jobs: &[Job]) -> String {
    if jobs.is_empty() {
        return "No matching jobs from the previous result.".to_string();
    }
    let lead = match jobs.len() {
        1 => "Here's the summary:".to_string(),
        n => format!("Here are the summaries for the {n} you asked about:"),
    };
    let mut lines = vec![lead];
    for j in jobs {
        let company = if j.company.is_empty() { "Unknown company" } else { j.company.as_str() };
        let summary = if j.summary.trim().is_empty() {
            "No summary available."
        } else {
            j.summary.as_str()
        };
        lines.push(format!("\n{} — {}\n{}", j.title, company, summary));
    }
    lines.join("\n")
}

fn format_search_keyword_reply(query: &str, jobs: &[Job]) -> String {
    if jobs.is_empty() {
        return format!("No jobs found matching \"{query}\".");
    }
    let mut lines = vec![format!(
        "Found {} job{} matching \"{}\":",
        jobs.len(),
        if jobs.len() == 1 { "" } else { "s" },
        query
    )];
    for (i, j) in jobs.iter().enumerate() {
        let company = if j.company.is_empty() { "Unknown company" } else { j.company.as_str() };
        lines.push(format!("{}. {} — {}", i + 1, j.title, company));
    }
    lines.push("Open any card below for full details.".to_string());
    lines.join("\n")
}

fn extract_query_terms(lower: &str) -> Vec<String> {
    let stop_words = [
        "can", "you", "provide", "the", "top", "best", "paying", "highest",
        "show", "me", "give", "find", "list", "from", "latest", "last", "scan",
        "job", "jobs", "listing", "listings", "role", "roles", "position", "positions",
        "result", "results", "all", "my", "a", "an", "for", "with", "and", "or",
        "in", "of", "to", "what", "are", "is", "it", "how", "new", "each",
        "option", "options", "recommend", "suggest", "match", "matches",
        "describe", "description", "descriptions", "summary", "summarize",
        "details", "them", "these", "those", "tell", "more", "about",
        "short", "their", "compare", "explain", "which", "one",
    ];
    lower
        .split_whitespace()
        .map(|t| t.trim_matches(|c: char| !c.is_alphanumeric()).to_string())
        .filter(|t| t.len() >= 2 && !stop_words.contains(&t.as_str()))
        .collect()
}

fn job_pay_score_usd_monthly(job: &Job) -> Option<f64> {
    let needs_fallback_parse = job.salary_min.is_none()
        || job.salary_currency.trim().is_empty()
        || job.salary_period.trim().is_empty();
    let parsed = if needs_fallback_parse {
        Some(parse_pay(&job.pay))
    } else {
        None
    };

    let min = job.salary_min.or_else(|| parsed.as_ref().and_then(|p| p.min))?;
    if min <= 0.0 {
        return None;
    }

    let currency = if job.salary_currency.trim().is_empty() {
        parsed.as_ref().map(|p| p.currency.to_uppercase()).unwrap_or_default()
    } else {
        job.salary_currency.to_uppercase()
    };
    let period = if job.salary_period.trim().is_empty() {
        parsed.as_ref().map(|p| p.period.to_lowercase()).unwrap_or_default()
    } else {
        job.salary_period.to_lowercase()
    };

    let monthly_amount = match period.as_str() {
        "hourly" => min * 160.0,
        "monthly" => min,
        _ => min,
    };

    let usd_monthly = match currency.as_str() {
        "PHP" => monthly_amount / 55.0,
        _ => monthly_amount,
    };
    Some(usd_monthly)
}

fn compare_jobs_for_ranking(a: &Job, b: &Job) -> Ordering {
    match (job_pay_score_usd_monthly(a), job_pay_score_usd_monthly(b)) {
        (Some(va), Some(vb)) => vb
            .partial_cmp(&va)
            .unwrap_or_else(|| b.scraped_at.cmp(&a.scraped_at)),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => b.scraped_at.cmp(&a.scraped_at),
    }
}

fn format_ranking_reply(jobs: &[Job], include_descriptions: bool, by_pay: bool) -> String {
    if jobs.is_empty() {
        return "No jobs matched your criteria. Try running a scan or adjusting your keywords.".to_string();
    }
    let header = if by_pay {
        format!("Top {} jobs by normalized pay:", jobs.len())
    } else {
        format!("Top {} recent jobs (pay data unavailable):", jobs.len())
    };
    let mut lines = vec![header];
    for (i, job) in jobs.iter().enumerate() {
        let pay_display = if job.pay.is_empty() { "-".to_string() } else { job.pay.clone() };
        lines.push(format!("{}. {} — {} ({})", i + 1, job.title, job.company, pay_display));
        if include_descriptions {
            lines.push(format!("   {}", short_description(&job.summary)));
        }
    }
    lines.push("Open any card below for full details.".to_string());
    lines.join("\n")
}

fn format_describe_reply(jobs: &[Job]) -> String {
    if jobs.is_empty() {
        return "No jobs found to describe.".to_string();
    }
    let mut lines = vec![format!("Descriptions for {} jobs:", jobs.len())];
    for (i, job) in jobs.iter().enumerate() {
        lines.push(format!("{}. {}", i + 1, job.title));
        lines.push(format!("   {}", short_description(&job.summary)));
    }
    lines.push("Open any card below for full details.".to_string());
    lines.join("\n")
}

fn build_ollama_system_prompt(jobs: &[Job]) -> String {
    let job_context: String = jobs.iter().take(25).map(|j| {
        let brief = if j.summary.is_empty() {
            "-".to_string()
        } else {
            let cleaned = sanitize_text(&j.summary);
            if cleaned.chars().count() > 150 {
                let mut s = String::new();
                for ch in cleaned.chars().take(147) { s.push(ch); }
                s.push_str("...");
                s
            } else {
                cleaned
            }
        };
        format!(
            "- [job_id={}] Title: {} | Company: {} | Pay: {} | Type: {} | Keyword: {} | Posted: {} | URL: {} | Summary: {}",
            j.id.unwrap_or_default(),
            j.title,
            if j.company.is_empty() { "-" } else { &j.company },
            if j.pay.is_empty() { "-" } else { &j.pay },
            if j.job_type.is_empty() { "-" } else { &j.job_type },
            if j.keyword.is_empty() { "-" } else { &j.keyword },
            if j.posted_at.is_empty() { "-" } else { &j.posted_at },
            if j.url.is_empty() { "-" } else { &j.url },
            brief
        )
    }).collect::<Vec<_>>().join("\n");

    format!(
        "{}\n\nAdditional rules:\n\
- ONLY use the local job context below to answer. Do not invent or assume data not present.\n\
- When listing jobs, always include the exact Title as shown in context.\n\
- If the answer is uncertain or data is missing, say what is missing and suggest a scan.\n\
- Use bullets/numbered lists for multiple items; avoid long paragraphs.\n\
- No fluff, no generic disclaimers, no mentions of external platforms.\n\
\n\
Local job context ({} jobs):\n{}",
        system_prompt_for_job_chat(),
        jobs.len(),
        if job_context.is_empty() { "No jobs available in current filter scope.".to_string() } else { job_context }
    )
}

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
        .append_ai_message(convo_id, "user", &message, "{}", &[], &now)
        .map_err(|e| e.to_string())?;

    let history = state.db.get_ai_messages(convo_id).map_err(|e| e.to_string())?;

    if is_prompt_injection_attempt(&message) {
        let reply = "I can’t follow that request. I only operate within this app’s scope and policy.".to_string();
        let latency = started.elapsed().as_millis() as i64;
        let _ = state.db.log_ai_run(&AiRunLog {
            task_type: "chat",
            latency_ms: latency,
            status: "blocked_injection",
            error: Some("prompt_injection_detected"),
            created_at: &now,
            route: Some("blocked_injection"),
            ..Default::default()
        });
        state
            .db
            .append_ai_message(
                convo_id,
                "assistant",
                &reply,
                &assistant_meta("local", Some("blocked_injection"), None),
                &[],
                &now,
            )
            .map_err(|e| e.to_string())?;
        return Ok(AiChatResponse {
            conversation_id: convo_id,
            reply,
            cards: None, error: None });
    }

    if !is_app_scope_query(&message, &history) {
        let reply = out_of_scope_reply();
        let latency = started.elapsed().as_millis() as i64;
        let _ = state.db.log_ai_run(&AiRunLog {
            task_type: "chat",
            latency_ms: latency,
            status: "blocked_scope",
            error: Some("out_of_scope_query"),
            created_at: &now,
            route: Some("blocked_scope"),
            ..Default::default()
        });
        state
            .db
            .append_ai_message(
                convo_id,
                "assistant",
                &reply,
                &assistant_meta("local", Some("blocked"), None),
                &[],
                &now,
            )
            .map_err(|e| e.to_string())?;
        return Ok(AiChatResponse {
            conversation_id: convo_id,
            reply,
            cards: None, error: None });
    }

    let cfg = state.db.get_ai_runtime_config().map_err(|e| e.to_string())?;
    let limit_history = 8usize;
    let recent: Vec<AiMessage> = if history.len() > limit_history {
        history[(history.len() - limit_history)..].to_vec()
    } else {
        history
    };

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
    let intent_str = match &intent {
        ChatIntent::Ranking { .. } => "ranking",
        ChatIntent::FollowUp => "followup",
        ChatIntent::Describe { .. } => "describe",
        ChatIntent::SearchKeyword { .. } => "search_keyword",
        ChatIntent::General => "general",
    };

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
                match semantic_search_fallback(state.inner(), &cfg, query, &fts_ids, want).await {
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
