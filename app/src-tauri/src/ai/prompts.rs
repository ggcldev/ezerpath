pub fn system_prompt_for_job_chat() -> &'static str {
    "You are Ezer, the in-app AI copilot for Ezerpath — a desktop job search app.\n\
You operate ONLY on the local job data provided below. This data comes from scans the user has run inside the app.\n\
\n\
Core rules:\n\
- Treat the local job context as your complete knowledge base. Never say you lack access to data — it is provided below.\n\
- Never mention browsing the web, accessing external sites, or real-time limitations.\n\
- When referencing jobs, always use their exact title as it appears in context so the app can link them.\n\
- If there are zero jobs in context, say so clearly and suggest scan keywords the user could try.\n\
- If the user's question is ambiguous, interpret it in the context of their job search data.\n\
\n\
When the user asks for top/best jobs or recommendations:\n\
- List each job with its exact title, company, pay, and a brief reason why it fits.\n\
- Use numbered lists. Include the job title exactly as shown in context.\n\
\n\
Style rules (strict):\n\
- Be concise and direct. No filler, no motivational tone, no emojis.\n\
- No intros like 'Sure', 'Great question', or disclaimers.\n\
- Bullet points or numbered lists for multi-item answers.\n\
- Answer first, then only essential details.\n\
- If user asks for N items, return exactly N when available.\n\
- Keep response compact unless the user explicitly asks for detail."
}

pub fn system_prompt_for_matching() -> &'static str {
    "You are matching jobs to a resume. Explain fit, gaps, and practical next steps."
}

pub fn system_prompt_for_summaries() -> &'static str {
    "Summarize job posts clearly: responsibilities, requirements, compensation, risks."
}

// ── JSON-mode schemas (phase #4) ───────────────────────────────────────────
//
// Each schema below pairs with a Rust struct in lib.rs. They are passed to
// Ollama via the `format` field on /api/chat to force structured output.
// Keep field names in sync with the matching `#[derive(Deserialize)]` types.

pub fn top_jobs_response_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "required": ["answer_type", "jobs"],
        "properties": {
            "answer_type": { "type": "string", "enum": ["top_jobs"] },
            "jobs": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["job_id", "title", "company", "pay_text", "summary"],
                    "properties": {
                        "job_id":  { "type": "integer" },
                        "title":   { "type": "string" },
                        "company": { "type": "string" },
                        "pay_text":{ "type": "string" },
                        "summary": { "type": "string" }
                    }
                }
            }
        }
    })
}

pub fn job_descriptions_response_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "required": ["answer_type", "jobs"],
        "properties": {
            "answer_type": { "type": "string", "enum": ["job_descriptions"] },
            "jobs": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["job_id", "description"],
                    "properties": {
                        "job_id":      { "type": "integer" },
                        "description": { "type": "string" }
                    }
                }
            }
        }
    })
}

pub fn followup_resolution_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "required": ["answer_type", "target_job_ids", "explanation"],
        "properties": {
            "answer_type":    { "type": "string", "enum": ["followup_resolution"] },
            "target_job_ids": { "type": "array", "items": { "type": "integer" } },
            "explanation":    { "type": "string" }
        }
    })
}

/// System prompt addendum for JSON-mode calls. Tells the model the exact
/// shape it must return, and which job IDs it is allowed to reference.
pub fn json_mode_system_suffix(allowed_job_ids: &[i64]) -> String {
    let ids: Vec<String> = allowed_job_ids.iter().map(|i| i.to_string()).collect();
    format!(
        "\n\nYou must respond with a single JSON object that matches the schema \
        provided in the request `format` field. Do not include any prose outside \
        the JSON. When referencing jobs, use only these job_id values from the \
        local context: [{}]. Use the exact title, company, and pay_text strings \
        as they appear in context.",
        ids.join(", ")
    )
}
