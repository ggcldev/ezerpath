# AI Copilot Implementation Plan (Ollama + Sentence Transformers)

## 1) Codebase Scan Summary (Current State)

This plan is based on the current codebase structure and active app architecture.

### Core app stack already in place
- Desktop app shell: Tauri 2 (`app/src-tauri`)
- Frontend: SolidJS + TypeScript (`app/src`)
- Local DB: SQLite via `rusqlite` (`app/src-tauri/src/db/mod.rs`)
- Crawler: Rust + `reqwest` + `scraper` (`app/src-tauri/src/crawler/mod.rs`)
- Existing IPC boundary: Tauri commands in `app/src-tauri/src/lib.rs`

### Existing product surfaces relevant to AI
- Job list/table views: `app/src/views/JobsView.tsx`, `app/src/views/WatchlistView.tsx`
- Scan management view: `app/src/views/ScanView.tsx`
- Job detail drawer (already rich context): `app/src/components/JobDetailsDrawer.tsx`
- Sidebar navigation currently supports `scan`, `jobs`, `watchlist`: `app/src/components/Sidebar.tsx`

### Existing backend data model
- `jobs`, `keywords`, `runs` tables exist.
- No AI, embedding, resume, or conversation tables yet.

### Existing non-Tauri crawler code
- Legacy Scrapy project exists in root (`crawler/`, `tests/`) but production desktop flow is Rust/Tauri.
- AI integration should be implemented in Tauri app path first.

---

## 2) Target AI Capability Scope

Implement an in-app AI Copilot that can:
1. Answer natural-language questions about scanned jobs.
2. Accept a user resume upload and rank best matching jobs from local scans.
3. If matches are weak/empty, suggest new scan keywords and optionally trigger a scan.
4. Summarize single jobs or compare multiple jobs.

---

## 3) Proposed Technical Architecture

## 3.1 Inference + Embeddings
- Generation/reasoning: **Ollama** (local model serving).
- Semantic matching: **Sentence Transformers** embeddings.

Recommended initial models:
- Ollama generation model: `qwen2.5:7b-instruct` (or similar instruct model available locally).
- Embedding model: `all-MiniLM-L6-v2` first (fast), optional upgrade to `bge-m3` later.

## 3.2 Service boundaries
- Rust (Tauri backend) remains source of truth and orchestration layer.
- Python microservice (local only) handles embeddings initially:
  - Accepts text batch
  - Returns vectors
- Rust stores vectors in SQLite and performs retrieval/rerank orchestration.

Rationale:
- Fastest delivery with highest quality embedding ecosystem.
- Keeps core app architecture stable.
- Allows later migration to pure-Rust embeddings if needed.

## 3.3 Data flow
1. Crawl jobs -> store in `jobs`.
2. Embed job text -> store vectors.
3. User uploads resume -> parse text -> embed resume.
4. Similarity search -> top-N candidate jobs.
5. Rust builds grounded prompt (top-N + metadata).
6. Ollama returns answer/summaries/recommendations.

---

## 4) Database Changes (SQLite)

Add new tables/migrations in `app/src-tauri/src/db/mod.rs`:

- `resume_profiles`
  - `id`, `name`, `source_file`, `raw_text`, `normalized_text`, `created_at`, `updated_at`, `is_active`

- `job_embeddings`
  - `job_id` (FK jobs.id), `model_name`, `vector` (BLOB or JSON text), `updated_at`
  - unique `(job_id, model_name)`

- `resume_embeddings`
  - `resume_id`, `model_name`, `vector`, `updated_at`

- `ai_conversations`
  - `id`, `created_at`, `updated_at`, `title`

- `ai_messages`
  - `id`, `conversation_id`, `role` (user/assistant/system), `content`, `created_at`, `meta_json`

- `ai_runs` (observability)
  - `id`, `task_type` (chat/match/summarize/suggest_keywords), `latency_ms`, `status`, `error`, `created_at`

Note: if vector scale grows, move to `sqlite-vec` extension later.

---

## 5) API/IPC Additions (Tauri Commands)

Add commands in `app/src-tauri/src/lib.rs` (and corresponding service modules):

1. Resume lifecycle
- `upload_resume(path_or_bytes)`
- `list_resumes()`
- `set_active_resume(resume_id)`

2. Embedding/index lifecycle
- `index_jobs_embeddings()`
- `index_resume_embedding(resume_id)`
- `embedding_index_status()`

3. Copilot chat
- `ai_chat(conversation_id, message, filters)`
- `ai_get_conversation(conversation_id)`
- `ai_list_conversations()`

4. Matching + recommendations
- `ai_match_jobs(resume_id, filters)`
- `ai_suggest_keywords(resume_id, current_keywords, jobs_context)`

5. Summarization
- `ai_summarize_job(job_id)`
- `ai_compare_jobs(job_ids[])`

6. Optional action integration
- `ai_start_scan_with_keywords(keywords[], days)` (wrapper around current crawl flow)

---

## 6) Frontend UX Plan

## 6.1 New navigation target
- Extend `View` union in `Sidebar.tsx` and `App.tsx` with `"copilot"`.
- Add `Copilot` nav item in sidebar.

## 6.2 New view/components
Create:
- `app/src/views/CopilotView.tsx`
- `app/src/components/ai/ChatPanel.tsx`
- `app/src/components/ai/ResumeUploader.tsx`
- `app/src/components/ai/MatchResults.tsx`
- `app/src/components/ai/SuggestedKeywords.tsx`

## 6.3 UX requirements
- Chat stream-like feel (snappy, non-blocking UI)
- Resume upload + active profile indicator
- "Best matches" list with score + explanation
- "No strong match" state with suggested keywords + one-click scan
- Quick actions from job drawer:
  - "Summarize this job"
  - "Find similar jobs"

---

## 7) Ollama + Embeddings Runtime Design

## 7.1 Ollama integration (Rust)
- Add Rust HTTP client module: `app/src-tauri/src/ai/ollama.rs`
- Endpoints:
  - `/api/chat` for Q&A/summaries
  - Optional `/api/embeddings` only for fallback
- Config in app settings/env:
  - `OLLAMA_BASE_URL` (default `http://127.0.0.1:11434`)
  - model name
  - timeout/max tokens/temperature

## 7.2 Sentence Transformers integration (local service)
Create Python service under repo root:
- `ai_service/requirements.txt`
- `ai_service/server.py`
- `ai_service/embedder.py`

Endpoints:
- `POST /embed` -> `{ texts: string[] } => { vectors: number[][], model: string }`

Execution model:
- Start service on demand from Tauri Rust if not running.
- Keep local-only (`127.0.0.1`) and no cloud dependency.

---

## 8) Ranking Strategy (Initial)

For each job:
1. Compute cosine similarity between resume vector and job vector.
2. Add lightweight heuristics:
- keyword overlap boost
- title-role alignment boost
- optional pay/range relevance
3. Select top-N (e.g., 20).
4. Ask Ollama to rerank top-N and provide explanations.

Output fields:
- `match_score`
- `why_match`
- `missing_requirements`
- `recommended_next_steps`

---

## 9) Security / Privacy / Reliability

- Local-first by default.
- Explicit user notice before any external AI endpoint usage.
- Sanitize resume text in logs (or disable body logging by default).
- Timeouts + retries for Ollama and embed service.
- Graceful fallback: if AI is unavailable, app still works with non-AI job browsing.

---

## 10) Delivery Phases and TODO Checklist

## Phase A — Foundation (AI plumbing)
- [ ] Create `app/src-tauri/src/ai/` module structure (`mod.rs`, `ollama.rs`, `ranking.rs`, `prompts.rs`)
- [ ] Add DB migrations for resumes, embeddings, conversations, ai_runs
- [ ] Add Tauri commands skeleton for AI operations
- [ ] Add settings for AI runtime config (base URL, model)
- [ ] Add health-check command for Ollama

## Phase B — Resume + Embeddings
- [ ] Implement resume upload and local text extraction (`pdf/docx/txt`)
- [ ] Store normalized resume profile in SQLite
- [ ] Build local embedding service (`ai_service/`) with sentence-transformers
- [ ] Add job embedding indexing flow (batch + incremental)
- [ ] Add resume embedding generation flow
- [ ] Add index status and reindex commands

## Phase C — Match + Suggest
- [ ] Implement similarity retrieval + heuristic scoring
- [ ] Implement Ollama rerank/explanation prompt
- [ ] Add "no strong matches" detection threshold
- [ ] Implement keyword suggestion prompt from resume + current jobs/keywords
- [ ] Add one-click trigger for new scan from suggestions

## Phase D — Copilot UI
- [ ] Add `copilot` navigation in sidebar/app routing
- [ ] Build `CopilotView` shell with chat + resume + match panels
- [ ] Hook chat send/receive to new IPC commands
- [ ] Add UI state for loading/errors/retry
- [ ] Add summary actions from jobs and drawer

## Phase E — Summaries + Comparison
- [ ] Implement single-job summary command
- [ ] Implement multi-job comparison command
- [ ] Add copy/export from AI responses
- [ ] Add citations/context preview (which jobs were used)

## Phase F — Hardening
- [ ] Add unit tests for ranking math and prompt builders
- [ ] Add integration tests for AI command flows (with mock Ollama)
- [ ] Add local metrics/logging for latency and failures
- [ ] Add fallback UX when Ollama/embed service is offline
- [ ] Add documentation: setup, model requirements, troubleshooting

---

## 11) File-Level Implementation Map

Backend (Rust):
- `app/src-tauri/src/lib.rs` (new commands + app state wiring)
- `app/src-tauri/src/db/mod.rs` (new tables and queries)
- `app/src-tauri/src/ai/mod.rs` (new)
- `app/src-tauri/src/ai/ollama.rs` (new)
- `app/src-tauri/src/ai/ranking.rs` (new)
- `app/src-tauri/src/ai/prompts.rs` (new)

Frontend (Solid):
- `app/src/components/Sidebar.tsx` (add Copilot nav)
- `app/src/App.tsx` (route + resources)
- `app/src/views/CopilotView.tsx` (new)
- `app/src/components/ai/*` (new components)
- Optional integration in `app/src/components/JobDetailsDrawer.tsx` for quick AI actions

Local embedding service (Python):
- `ai_service/server.py` (new)
- `ai_service/embedder.py` (new)
- `ai_service/requirements.txt` (new)

---

## 12) Recommended Execution Order (Pragmatic)

1. Phase A + minimal Phase B (resume upload + embed API working)
2. Phase C baseline matching (without rerank first)
3. Phase D UI integration for match + chat shell
4. Add Ollama rerank + summaries
5. Add keyword suggestions + one-click scan
6. Hardening/testing/documentation

This order keeps user-visible value shipping early while reducing integration risk.
