# Ezerpath — v2 Architecture Upgrade Roadmap

Living checklist for the upgrades agreed on after the GPT-5 architecture review. Each item lists **scope**, **dependencies**, **files touched**, **plan**, and **acceptance criteria** so we can pause mid-flight and resume without re-deriving context.

**Order matters.** Each step produces data or scaffolding the next one needs. Don't reorder without checking the dependency notes — you'll end up doing #5 with no telemetry to feed it.

> Status legend: ☐ todo · ⏳ in progress · ✅ done · ⏸ paused · ✗ dropped

---

## #1 — Tauri Channel for scan progress ✅

**Why first:** worst UX in the app. `crawl_jobs` blocks on one awaited promise for ~30–100s with zero feedback. Fixing it requires no DB changes and no IPC contract churn for any *other* command, so it's the lowest-risk highest-leverage change.

**Dependencies:** none.

**Files touched:**
- `app/src-tauri/src/crawler/mod.rs` — add `ScanProgress` enum, thread an `Option<&Channel<ScanProgress>>` through `crawl_keyword`
- `app/src-tauri/src/lib.rs` — extract `run_crawl_inner(&AppState, days, Option<&Channel>)`, change `crawl_jobs` signature to take a `Channel<ScanProgress>`, route `ai_start_scan_with_keywords` through the same helper with `None`
- `app/src/views/ScanView.tsx` — define `ScanProgress` discriminated union, create `Channel` per scan, render real progress

**Plan:**
1. Define `pub enum ScanProgress` in `crawler/mod.rs` with serde tag `"kind"`, rename_all `snake_case`, variants:
   - `Started { run_id, total_keywords, keywords }`
   - `KeywordStarted { keyword, index, total }`
   - `Page { keyword, page, found }`
   - `KeywordCompleted { keyword, found, new, pages }`
   - `Completed { run_id, total_found, total_new }`
   - `Failed { run_id, error }`
2. Add `on_progress: Option<&tauri::ipc::Channel<ScanProgress>>` to `crawl_keyword`. Emit `Page` after each successfully ingested page.
3. In `lib.rs`, extract a private `async fn run_crawl_inner(state: &AppState, days: Option<u32>, on_progress: Option<&Channel<ScanProgress>>) -> Result<Vec<CrawlStats>, String>`. Move the lock + keyword loop in. Emit `Started` / `KeywordStarted` / `KeywordCompleted` / `Completed` / `Failed` from here.
4. `crawl_jobs(state, days, on_progress: Channel<ScanProgress>)` becomes a one-line wrapper that calls `run_crawl_inner(&state, days, Some(&on_progress))`.
5. `ai_start_scan_with_keywords` calls `run_crawl_inner(&state, days, None)` so AI scans still work without a frontend channel.
6. Frontend: `ScanView.tsx` constructs `new Channel<ScanProgress>()` per scan, attaches `onmessage`, passes `onProgress: channel` in the `invoke` args. Replace the fake `width: 55%` pulse with a computed percentage based on `keyword_index / total + page_progress / total`. Show the current keyword and page count as text.

**Acceptance criteria:**
- [ ] Clicking **Scan Now** shows the current keyword updating live (e.g. "scanning 'seo specialist' — page 2/5").
- [ ] Progress bar advances monotonically and reaches 100% when scan completes.
- [ ] On scan failure, the error toast still fires and the bar stops where it failed.
- [ ] `ai_start_scan_with_keywords` still works (silent, no channel needed).
- [ ] `cargo check` clean, `npm test` green, manual scan completes end-to-end.

---

## #2 — Telemetry breakdown on `ai_runs` ✅

**Why second:** every later step is debugged or measured against this table. Building #3, #4, #5 without these columns means flying blind.

**Dependencies:** none — but #5 requires this to exist first.

**Files touched:**
- `app/src-tauri/src/db/mod.rs` — add columns via `ALTER TABLE` migrations (schema is already migration-friendly)
- `app/src-tauri/src/lib.rs` — `log_ai_run` call sites in `ai_chat`; capture timings around retrieval and Ollama
- `app/src-tauri/src/db/mod.rs` — extend `log_ai_run` signature to accept the new fields

**Plan:**
1. Migrations:
   ```sql
   ALTER TABLE ai_runs ADD COLUMN intent TEXT;
   ALTER TABLE ai_runs ADD COLUMN route TEXT;
   ALTER TABLE ai_runs ADD COLUMN candidate_job_ids TEXT;  -- JSON array, set BEFORE LLM call
   ALTER TABLE ai_runs ADD COLUMN final_job_ids TEXT;      -- JSON array, set AFTER (matches assistant message linked_job_ids)
   ALTER TABLE ai_runs ADD COLUMN retrieval_ms INTEGER;
   ALTER TABLE ai_runs ADD COLUMN llm_ms INTEGER;
   ```
2. In `ai_chat`, time the SQL/embedding retrieval phase and the Ollama phase separately. Snapshot `candidate_job_ids` immediately after retrieval, before any LLM call. Snapshot `final_job_ids` from the cards or linked_ids that get persisted.
3. `intent` = router classification (`ranking | followup | describe | search_keyword | general`). `route` = which path actually executed (`sql_first | local_describe | ollama_followup | ollama_streaming | …`). They diverge when routes fall through (e.g. ranking found no SQL hits and fell to Ollama).
4. Keep the existing `log_ai_run` call shape; extend it with the extra params or add a new `log_ai_run_v2` if migration is messy.

**Acceptance criteria:**
- [ ] Every `ai_chat` invocation writes a row with all six new fields populated.
- [ ] Diffing `candidate_job_ids` vs `final_job_ids` for a sample chat shows whether the LLM kept all retrieved items.
- [ ] Existing `ai_runs` rows survive the migration (NULL columns are fine).
- [ ] No regressions in `ai_chat` happy path or fallback paths.

---

## #3 — FTS5 + `search_keyword` intent route ✅

**Why third:** real keyword recall. Current `db::get_jobs` uses `LIKE %kw%` which misses morphology, ordering, and ranking. SQLite ships FTS5; the cost is one virtual table and a trigger, and it gives BM25 ranking for free.

**Dependencies:** #2 (so the new `route` value can be telemetered).

**Files touched:**
- `app/src-tauri/src/db/mod.rs` — `CREATE VIRTUAL TABLE jobs_fts USING fts5(...)`, triggers on `jobs` insert/update/delete to keep FTS in sync, new query `search_jobs_fts(query, limit)`
- `app/src-tauri/src/lib.rs` — new intent variant `ChatIntent::SearchKeyword`, route in `ai_chat`
- `app/src-tauri/src/lib.rs` — extend `classify_intent` to recognize keyword-search phrasing

**Plan:**
1. Migration: `CREATE VIRTUAL TABLE IF NOT EXISTS jobs_fts USING fts5(title, company, summary, content='jobs', content_rowid='id');` plus `INSERT INTO jobs_fts(jobs_fts) VALUES('rebuild');` once.
2. Triggers: `AFTER INSERT/UPDATE/DELETE ON jobs` to keep `jobs_fts` mirrored.
3. New DB method `search_jobs_fts(&self, query: &str, limit: usize) -> Result<Vec<Job>>`. Use `bm25(jobs_fts)` for ordering. Escape query for FTS5 syntax.
4. Add `ChatIntent::SearchKeyword { query }` and a heuristic in `classify_intent`: detect "find/search/show me jobs about/for/with X" patterns that aren't ranking questions.
5. Wire the route: SearchKeyword → `db.search_jobs_fts(query, 10)` → format as cards (no LLM needed for the listing itself, only for an optional one-line lead-in).

**Acceptance criteria:**
- [ ] `db::search_jobs_fts("seo outreach")` returns ranked results, missing "outreach SEO" handled by FTS tokenizer.
- [ ] A chat like "find jobs about link building outreach" routes to `search_keyword`, not `general`.
- [ ] `ai_runs.route = "search_keyword"` is logged.
- [ ] `INSERT INTO jobs ...` from the crawler keeps `jobs_fts` in sync (test by inserting and immediately querying FTS).
- [ ] No regressions to existing intent routes.

---

## #4 — JSON-schema outputs for Ranking / Describe / FollowUp ✅

**Why fourth:** the structured paths already produce cards. Forcing Ollama JSON mode for them eliminates parser fragility, makes follow-ups deterministic (job_ids come from the LLM, not regex over prose), and produces stable shapes the frontend can render confidently. **Skip the General path** — schema-constraining free-form chat degrades quality.

**Dependencies:** #2 (so we can measure schema-mode regressions). Helpful but not required: #3.

**Files touched:**
- `app/src-tauri/src/ai/ollama.rs` — add `chat_json(cfg, messages, schema)` variant that sets Ollama's `format` field
- `app/src-tauri/src/ai/prompts.rs` — define schemas as `serde_json::Value` constants
- `app/src-tauri/src/lib.rs` — Ranking, Describe, FollowUp paths call `chat_json` and `serde_json::from_str` the result; format cards from typed structs instead of regex

**Plan:**
1. Add `OllamaChatRequest` field `format: Option<serde_json::Value>` (omitted by serde when None).
2. New method `OllamaClient::chat_json<T: DeserializeOwned>(cfg, messages, schema) -> Result<T>` — same streaming machinery as `chat`, but the `format` field is the JSON schema and the final accumulated string is parsed into `T`.
3. Define three Rust structs + matching JSON schemas:
   - `TopJobsResponse { answer_type, jobs: [{ job_id, title, company, pay_text, summary }] }`
   - `JobDescriptionsResponse { answer_type, jobs: [{ job_id, description }] }`
   - `FollowUpResolution { answer_type, target_job_ids: [int], explanation }`
4. Replace prose-parsing in the three structured intent branches with typed deserialization.
5. Ensure `temperature: 0.0` for these calls (per Ollama structured-output guidance).
6. Leave the General path on the existing streaming `chat()` — free-form text.

**Acceptance criteria:**
- [ ] A "top 5 high paying jobs" prompt returns a parseable `TopJobsResponse`, frontend renders cards from typed data.
- [ ] A "describe the second one" follow-up resolves via `FollowUpResolution.target_job_ids` instead of regex over the previous prose.
- [ ] If Ollama returns malformed JSON, we surface a clear error and fall back to the existing free-form path (graceful degradation).
- [ ] General chat is unchanged (still streaming, still free-form).
- [ ] `ai_runs.route` reflects schema vs free-form path.

---

## #5 — Golden-query evaluation harness ✅

**Why fifth:** turns prompt-tweaking from guesswork into engineering. With #2 in place, this is the thing that lets us actually measure whether #3 and #4 helped.

**Dependencies:** #2 (telemetry columns), #3 (so search routes are real), #4 (so outputs are parseable).

**Files touched:**
- `app/src-tauri/eval/golden_queries.json` — checked-in question/expected-job-id pairs
- `app/src-tauri/eval/snapshot.sql` — frozen DB seed (small subset of real jobs)
- `app/src-tauri/tests/eval.rs` — Rust integration test that loads the snapshot, runs each query, compares `candidate_job_ids` against expected, asserts recall@k threshold

**Plan:**
1. Pick 15–20 real questions from your own use, each with 1–5 expected `job_id`s in a fixed snapshot. Cover all intents: ranking, describe, follow-up, search_keyword.
2. Snapshot a small DB (~50 jobs) to a .sql or .db file under `eval/`. This is the corpus tests run against.
3. Test harness:
   - Spin up an in-memory or temp-file DB with the snapshot loaded
   - Run each query through `ai_chat` (or directly through the intent router for retrieval-only eval)
   - Read `ai_runs.candidate_job_ids` and `final_job_ids`
   - Compute recall@k (retrieval) and exact-match rate (generation)
   - Fail the test if recall@5 < 0.8 or any answer drops a previously-passing query
4. Add a `cargo test --test eval` invocation to the dev workflow.
5. Document in README how to add a new golden query when you find a regression.

**Acceptance criteria:**
- [ ] `cargo test --test eval` runs and reports per-query recall@k.
- [ ] Adding a deliberately broken prompt makes the eval fail loudly.
- [ ] Eval set covers all five intent routes.
- [ ] Eval runs in < 60s on a warm Ollama (or has a `--llm-skip` mode that only checks retrieval).

---

## Out of scope (consciously, for now)

These are good ideas but earn their complexity only after #1–#5 prove insufficient. Don't pull them forward without a measured reason.

- **Module split of `lib.rs`** — `commands/{scan,ai,jobs,settings}.rs`. `lib.rs` is 1336 lines, not catastrophic. Do after #5 once we know the natural seams.
- **Background-job table with retries / cancel** — only worth it when there's a *second* long-running operation besides crawl. `crawl_lock + Channel` is enough for one.
- **Cross-encoder reranking** — only if eval shows cosine similarity is failing. For ~thousands of jobs with rich metadata, it probably isn't.
- **Schema-constrained General chat** — would degrade output quality. Free-form is correct for that path.
- **scan_runs / scan_logs separation** — `runs` already has status, error, finished_at. Sufficient until we need streaming logs.
- **TanStack-style query layer in the frontend** — current `invoke` + signal pattern is fine at this scale.

---

## Notes & decisions log

- **2026-04-12** — chose Tauri **Channels** (not events) for scan progress per Tauri 2 docs. Channels are the documented streaming primitive; events are for small notifications.
- **2026-04-12** — chose **idle-gap timeout** for Ollama streaming over total-time wall clock. Already shipped (commit `f9b732a`).
- **2026-04-12** — chose to **scope JSON schemas to structured intent paths only**, leave General chat free-form.
- **2026-04-12** — chose to **store `candidate_job_ids` before LLM call**, separately from `final_job_ids`, to enable retrieval-vs-generation diagnosis.
