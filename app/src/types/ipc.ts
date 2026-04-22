export const SUPPORTED_EMBEDDING_MODEL = "all-MiniLM-L6-v2";

// Frontend fallback defaults mirror Rust `AiRuntimeConfig::default`.
// Persisted DB config replaces these after `get_ai_runtime_config` loads.
export const DEFAULT_AI_RUNTIME_CONFIG: AiRuntimeConfig = {
  ollama_base_url: "http://127.0.0.1:11434",
  ollama_model: "qwen2.5:7b-instruct",
  embedding_model: SUPPORTED_EMBEDDING_MODEL,
  temperature: 0.2,
  max_tokens: 1024,
  timeout_ms: 120000,
};

export interface Job {
  id: number;
  source: string;
  source_id: string;
  title: string;
  company: string;
  company_logo_url: string;
  pay: string;
  posted_at: string;
  url: string;
  summary: string;
  keyword: string;
  scraped_at: string;
  is_new: boolean;
  watchlisted: boolean;
  run_id: number | null;
  salary_min: number | null;
  salary_max: number | null;
  salary_currency: string;
  salary_period: string;
  normalized_pay_usd_hourly: number | null;
  normalized_pay_usd_monthly: number | null;
  pay_range: string;
  applied: boolean;
  job_type: string;
}

export interface ScanRun {
  id: number;
  started_at: string;
  keywords: string;
  total_found: number;
  total_new: number;
}

export interface CrawlStats {
  keyword: string;
  found: number;
  new: number;
  pages: number;
}

export type ScanProgress =
  | { kind: "started"; run_id: number; total_keywords: number; keywords: string[] }
  | { kind: "keyword_started"; keyword: string; index: number; total: number }
  | { kind: "page"; keyword: string; page: number; found: number }
  | { kind: "keyword_completed"; keyword: string; found: number; new: number; pages: number }
  | { kind: "completed"; run_id: number; total_found: number; total_new: number }
  | { kind: "failed"; run_id: number; error: string }
  | { kind: "bruntwork_keyword"; keyword: string; found: number; new: number };

export interface AiRuntimeConfig {
  ollama_base_url: string;
  ollama_model: string;
  embedding_model: string;
  temperature: number;
  max_tokens: number;
  timeout_ms: number;
}

export interface EmbeddingIndexStatus {
  jobs_total: number;
  jobs_indexed: number;
  resumes_total: number;
  resumes_indexed: number;
  active_embedding_model: string;
}

export interface BackendDiagnostics {
  state: "available" | "ready";
  ready: boolean;
  embedding_model: string;
  native_embedder_ready: boolean;
  embeddings_cache_dir: string;
  runtime_mode: "native";
}

export interface ResumeProfileSummary {
  id: number;
  name: string;
  source_file: string;
  created_at: string;
  updated_at: string;
  is_active: boolean;
}

export interface JobFacetCount {
  value: string;
  count: number;
}

export interface JobFilterOptions {
  keywords: JobFacetCount[];
  sources: JobFacetCount[];
  schedules: JobFacetCount[];
  pay_ranges: JobFacetCount[];
  latest_run_count: number;
}

export interface JobDetailsPayload {
  company: string;
  poster_name: string;
  company_logo_url: string;
  description: string;
  description_html: string;
  posted_at: string;
}

export interface AiConversation {
  id: number;
  title: string;
  created_at: string;
  updated_at: string;
}

export interface AiMessage {
  id: number;
  conversation_id: number;
  role: "user" | "assistant" | "system";
  content: string;
  created_at: string;
  meta_json: string;
}

export interface AiJobCard {
  job_id: number;
  title: string;
  company: string;
  pay: string;
  posted_at: string;
  url: string;
  logo_url: string;
}

export interface AiChatError {
  code: string;
  message: string;
}

export interface AiChatResponse {
  conversation_id: number;
  reply: string;
  cards?: AiJobCard[] | null;
  error?: AiChatError | null;
}
