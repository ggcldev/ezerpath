pub mod ollama;
pub mod prompts;
pub mod ranking;
pub mod sentence_service;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiRuntimeConfig {
    pub ollama_base_url: String,
    pub ollama_model: String,
    pub embedding_service_url: String,
    pub embedding_model: String,
    pub temperature: f32,
    pub max_tokens: u32,
    pub timeout_ms: u64,
}

impl Default for AiRuntimeConfig {
    fn default() -> Self {
        Self {
            ollama_base_url: "http://127.0.0.1:11434".to_string(),
            ollama_model: "qwen2.5:7b-instruct".to_string(),
            embedding_service_url: "http://127.0.0.1:8765".to_string(),
            embedding_model: "all-MiniLM-L6-v2".to_string(),
            temperature: 0.2,
            max_tokens: 1024,
            timeout_ms: 30_000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiHealth {
    pub ok: bool,
    pub message: String,
    pub model_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResumeProfile {
    pub id: i64,
    pub name: String,
    pub source_file: String,
    pub raw_text: String,
    pub normalized_text: String,
    pub created_at: String,
    pub updated_at: String,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingIndexStatus {
    pub jobs_total: i64,
    pub jobs_indexed: i64,
    pub resumes_total: i64,
    pub resumes_indexed: i64,
    pub active_embedding_model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingHealth {
    pub ok: bool,
    pub message: String,
    pub model_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConversation {
    pub id: i64,
    pub title: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiMessage {
    pub id: i64,
    pub conversation_id: i64,
    pub role: String,
    pub content: String,
    pub created_at: String,
    pub meta_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiChatFilters {
    pub keyword: Option<String>,
    pub watchlisted_only: Option<bool>,
    pub days_ago: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiChatResponse {
    pub conversation_id: i64,
    pub reply: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchJobResult {
    pub job_id: i64,
    pub score: f32,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeywordSuggestion {
    pub keyword: String,
    pub reason: String,
}
