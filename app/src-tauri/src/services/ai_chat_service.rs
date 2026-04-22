use crate::ai::{AiJobCard, AiMessage};
use crate::db::Job;

pub(crate) fn chat_title_from_query(message: &str) -> String {
    let normalized = message
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string();
    if normalized.is_empty() {
        return "New Chat".to_string();
    }

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

pub(crate) fn sanitize_text(raw: &str) -> String {
    if raw.trim().is_empty() {
        return String::new();
    }

    let mut out = String::with_capacity(raw.len());
    let mut in_tag = false;
    for ch in raw.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(crate) fn short_description(text: &str) -> String {
    let cleaned = sanitize_text(text);
    if cleaned.is_empty() {
        return "No short description available from this scan.".to_string();
    }

    for sep in [". ", "! ", "? ", ".\n", "!\n", "?\n"] {
        if let Some((first, _)) = cleaned.split_once(sep) {
            let first = first.trim();
            if first.len() >= 24 {
                return format!("{first}.");
            }
        }
    }
    let max = 170usize;
    if cleaned.chars().count() <= max {
        cleaned
    } else {
        let mut s = String::new();
        for ch in cleaned.chars().take(max - 1) {
            s.push(ch);
        }
        s.push('…');
        s
    }
}

pub(crate) fn out_of_scope_reply() -> String {
    "I’m Ezer, and I’m only made for this app. I can help with scanned jobs, resume matching, keyword suggestions, and job summaries inside Ezerpath.".to_string()
}

pub(crate) fn is_prompt_injection_attempt(message: &str) -> bool {
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

pub(crate) fn is_app_scope_query(message: &str, _history: &[AiMessage]) -> bool {
    let lower = message.to_lowercase();
    if lower.trim().is_empty() {
        return true;
    }

    let outside_terms = [
        "weather forecast",
        "news today",
        "sports score",
        "bitcoin price",
        "crypto price",
        "stock price",
        "movie review",
        "recipe for",
        "translate to",
        "math problem",
        "who is the president",
        "prime minister",
        "write me a poem",
        "tell me a joke",
    ];
    if outside_terms.iter().any(|t| lower.contains(t)) {
        return false;
    }

    true
}

pub(crate) fn response_violates_app_scope(reply: &str) -> bool {
    let lower = reply.to_lowercase();
    let bad_phrases = [
        "as a large language model",
        "as an ai language model",
        "i can't access",
        "i cannot access",
        "i don't have access",
        "i do not have access",
    ];
    bad_phrases.iter().any(|p| lower.contains(p))
}

pub(crate) fn get_linked_job_ids(history: &[AiMessage]) -> Vec<i64> {
    history
        .iter()
        .rev()
        .find(|m| m.role == "assistant")
        .and_then(|m| serde_json::from_str::<Vec<i64>>(&m.linked_job_ids_json).ok())
        .unwrap_or_default()
}

pub(crate) fn jobs_to_cards(jobs: &[Job]) -> Vec<AiJobCard> {
    jobs.iter()
        .map(|job| AiJobCard {
            job_id: job.id.unwrap_or_default(),
            title: job.title.clone(),
            company: if job.company.is_empty() {
                "Unknown company".to_string()
            } else {
                job.company.clone()
            },
            pay: if job.pay.is_empty() {
                "-".to_string()
            } else {
                job.pay.clone()
            },
            posted_at: if job.posted_at.is_empty() {
                "-".to_string()
            } else {
                job.posted_at.clone()
            },
            url: job.url.clone(),
            logo_url: job.company_logo_url.clone(),
        })
        .collect()
}

pub(crate) fn assistant_meta(
    provider: &str,
    scope: Option<&str>,
    cards: Option<&[AiJobCard]>,
) -> String {
    assistant_meta_full(provider, scope, cards, None)
}

pub(crate) fn assistant_meta_full(
    provider: &str,
    scope: Option<&str>,
    cards: Option<&[AiJobCard]>,
    error_code: Option<&str>,
) -> String {
    let mut meta = serde_json::Map::new();
    meta.insert(
        "provider".to_string(),
        serde_json::Value::String(provider.to_string()),
    );
    if let Some(scope_val) = scope {
        meta.insert(
            "scope".to_string(),
            serde_json::Value::String(scope_val.to_string()),
        );
    }
    if let Some(card_rows) = cards {
        if !card_rows.is_empty() {
            meta.insert(
                "cards".to_string(),
                serde_json::to_value(card_rows).unwrap_or(serde_json::Value::Array(vec![])),
            );
        }
    }
    if let Some(code) = error_code {
        meta.insert(
            "error_code".to_string(),
            serde_json::Value::String(code.to_string()),
        );
    }
    serde_json::Value::Object(meta).to_string()
}

pub(crate) fn compact_reply_text(reply: &str) -> String {
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
    const MAX_LEN: usize = 3000;
    if compact.len() > MAX_LEN {
        compact.truncate(MAX_LEN);
        compact.push_str("\n\n(Truncated for readability.)");
    }
    compact
}

pub(crate) fn extract_cards_from_reply(reply: &str, jobs: &[Job]) -> Vec<AiJobCard> {
    let lower_reply = reply.to_lowercase();
    let mut cards: Vec<AiJobCard> = Vec::new();

    for job in jobs {
        let job_id = job.id.unwrap_or_default();
        if cards.iter().any(|c| c.job_id == job_id) {
            continue;
        }
        let lower_title = job.title.to_lowercase();
        let title_match = lower_title.len() >= 5 && lower_reply.contains(&lower_title);
        let id_match = lower_reply.contains(&format!("job_id={}", job_id));
        if title_match || id_match {
            cards.push(AiJobCard {
                job_id,
                title: job.title.clone(),
                company: if job.company.is_empty() {
                    "Unknown company".to_string()
                } else {
                    job.company.clone()
                },
                pay: if job.pay.is_empty() {
                    "-".to_string()
                } else {
                    job.pay.clone()
                },
                posted_at: if job.posted_at.is_empty() {
                    "-".to_string()
                } else {
                    job.posted_at.clone()
                },
                url: job.url.clone(),
                logo_url: job.company_logo_url.clone(),
            });
            if cards.len() >= 10 {
                break;
            }
        }
    }

    cards
}
