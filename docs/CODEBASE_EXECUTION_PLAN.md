# Ezerpath Codebase Execution Plan

Concrete execution tracker based on the April 22, 2026 full-codebase review after `HEAD` `970c1c1`.

This file is for implementation tracking, not brainstorming. Each phase has a gate, exact tasks, file targets, and exit criteria. Do not start a later phase until the gate of the current phase is satisfied unless a task is explicitly marked independent.

## Status Legend

- `TODO` - not started
- `IN PROGRESS` - actively being worked
- `BLOCKED` - cannot continue until dependency/decision is resolved
- `DONE` - implemented and verified
- `DROPPED` - intentionally removed from scope

## Working Rules

1. Fix unsafe or semantically wrong behavior before structural refactors.
2. Decide the Python sidecar strategy before reorganizing the service layer.
3. Update CI/docs only after the runtime architecture is settled.
4. Every completed workstream must leave behind at least one regression test unless the task is documentation-only.

## Current Priority Order

1. Phase 1: Correctness and security stabilization
2. Phase 2: Sidecar architecture decision
3. Phase 3: Service and UI refactor after the decision
4. Phase 4: CI, docs, contracts, and regression coverage alignment

---

## Phase 1 - Correctness and Security Stabilization

**Goal:** remove the highest-risk behavior without waiting for broader architectural changes.

**Gate to exit Phase 1**

- The embedding model path is internally consistent.
- External job URLs are validated before any webview/network side effects.
- Resume import no longer exposes an arbitrary local-file read path.
- App startup no longer blocks on sidecar readiness.
- The watchlist no longer depends on the scan date-range resource.

### P1.1 - Fix embedding model contract

**Status:** `DONE`

**Why now**

The app currently stores and reads embeddings under `cfg.embedding_model`, but the native path always produces `AllMiniLML6V2`. That makes semantic search and resume matching unreliable if the setting changes.

**Primary files**

- `app/src-tauri/src/ai/mod.rs`
- `app/src-tauri/src/ai/sentence_service.rs`
- `app/src-tauri/src/ai/native_embedder.rs`
- `app/src-tauri/src/lib.rs`
- `app/src-tauri/src/db/mod.rs`
- `app/src/App.tsx`
- `app/src/components/SettingsPanel.tsx`
- `README.md`

**Exact tasks**

- [x] Decide the short-term contract:
  - Option A: support only one native embedding model now.
  - Option B: implement true multi-model native support now.
- [x] Recommended short-term path: choose Option A and explicitly enforce `all-MiniLM-L6-v2` end to end.
- [x] Reject unsupported values in `set_ai_runtime_config`.
- [x] Make embedding health/status return the actual active model, not the configured string when native mode is active.
- [x] Remove or disable free-form editing of `embedding_model` in the frontend until multi-model support is real.
- [x] Update indexing/retrieval code comments to reflect the enforced model contract.
- [x] Add backend tests that fail if a config save accepts an unsupported embedding model.

**Acceptance criteria**

- [x] A user cannot save a fake or unsupported embedding model.
- [x] `index_jobs_embeddings`, `index_resume_embedding`, `embedding_index_status`, `semantic_search_fallback`, and `ai_match_jobs` all use the same effective model namespace.
- [x] The health response names the real model in use.

**Suggested commit**

- `fix(ai): enforce a single supported embedding model contract`

### P1.2 - Validate job URLs before webview scraping

**Status:** `DONE`

**Why now**

`fetch_job_details` currently routes Bruntwork-like strings to the hidden webview before the crawler's allowlist check runs.

**Primary files**

- `app/src-tauri/src/lib.rs`
- `app/src-tauri/src/crawler/mod.rs`
- `app/src-tauri/src/crawler/webview_scraper.rs`

**Exact tasks**

- [x] Add an allowlist validation step at the start of the `fetch_job_details` command before any Bruntwork/webview branching.
- [x] Replace substring host detection with parsed-URL hostname matching.
- [x] Keep one allowlist implementation as the source of truth and call it from both command and crawler paths.
- [x] Add regression tests for:
  - valid `onlinejobs.ph`
  - valid `bruntworkcareers.co`
  - invalid hostname containing `bruntworkcareers.co` in query/path only
  - invalid scheme

**Acceptance criteria**

- [x] Invalid URLs are rejected before any hidden webview work begins.
- [x] Bruntwork detection is based on parsed host, not substring matching.
- [x] Existing valid job detail fetches still work.

**Suggested commit**

- `fix(security): validate job URLs before webview fallback`

### P1.3 - Harden resume import boundary

**Status:** `DONE`

**Why now**

`upload_resume_from_file` is currently too trusting. The renderer can ask the backend to read arbitrary local files, and the full contents are persisted and returned.

**Primary files**

- `app/src-tauri/src/lib.rs`
- `app/src-tauri/src/ai/sentence_service.rs`
- `app/src-tauri/src/ai/native_resume_parser.rs`
- `app/src-tauri/src/ai/mod.rs`
- `app/src-tauri/src/db/mod.rs`
- `app/src/App.tsx`
- `app/src/components/SettingsPanel.tsx`

**Exact tasks**

- [x] Narrow accepted file types in the backend to `.pdf`, `.docx`, `.txt` only. Remove extensionless fallback.
- [x] Remove the free-form "paste a file path" UX from the settings panel.
- [x] Introduce a backend-safe import flow:
  - frontend chooses a file with the native picker
  - backend imports and copies that file into an app-controlled resume import directory
  - extraction/indexing runs against the copied file, not arbitrary paths forever after
- [x] Split `ResumeProfile` responses into:
  - summary shape for list views
  - full-content shape only when explicitly needed
- [x] Change `list_resumes` to return metadata only, not `raw_text`/`normalized_text`.
- [x] Add backend tests for unsupported extension and summary-vs-full response behavior.

**Acceptance criteria**

- [x] Arbitrary local files cannot be read by passing a random path through IPC.
- [x] Resume listing endpoints do not expose full raw text by default.
- [x] Resume upload still works via the native file picker flow.

**Suggested commit**

- `fix(security): harden resume import and limit resume data exposure`

### P1.4 - Make sidecar startup truly non-blocking and improve diagnostics

**Status:** `DONE`

**Why now**

The app boot path currently waits on sidecar readiness despite the code comment claiming otherwise. The recent diagnostics work also drops real child-process stderr/stdout.

**Primary files**

- `app/src-tauri/src/lib.rs`
- `app/src-tauri/src/ai_service_manager.rs`
- `app/src/App.tsx`
- `app/src/components/SettingsPanel.tsx`

**Exact tasks**

- [x] Replace the `spawn(...).join()` startup path with a true background startup that does not block `setup()`.
- [x] Keep a service state handle in app state if needed, but do not gate the window boot on health readiness.
- [x] Capture child `stdout` and `stderr` into the same log file used by diagnostics.
- [x] Extend `BackendDiagnostics` with enough state to distinguish:
  - not configured / not packaged
  - spawning
  - ready
  - startup failed
  - timed out
- [x] Surface `backend_diagnostics` in the settings UI so startup failures are visible without reading logs manually.
- [x] Add a backend unit/integration test for the non-blocking startup path if feasible; otherwise add a targeted smoke harness and document the gap.

**Acceptance criteria**

- [x] The Tauri window opens without waiting up to 30 seconds for the sidecar.
- [x] The diagnostics UI shows actual startup state and log location.
- [x] Python tracebacks and bind/import failures appear in the log file.

**Suggested commit**

- `fix(runtime): make sidecar startup non-blocking and surface real diagnostics`

### P1.5 - Enable SQLite foreign keys explicitly

**Status:** `DONE`

**Why now**

The schema uses `ON DELETE CASCADE`, but the connection setup does not currently show `PRAGMA foreign_keys = ON`.

**Primary files**

- `app/src-tauri/src/db/mod.rs`

**Exact tasks**

- [x] Enable `PRAGMA foreign_keys = ON` when the SQLite connection is opened.
- [x] Add a regression test that creates dependent rows, deletes a parent row, and asserts child cleanup.
- [x] Verify `clear_all_jobs` and resume deletion flows no longer risk orphaned embedding rows.

**Acceptance criteria**

- [x] Cascading deletes behave as declared by schema.
- [x] Embedding counts do not drift after parent-row deletion.

**Suggested commit**

- `fix(db): enable foreign keys and test cascade behavior`

### P1.6 - Decouple watchlist from date-range-limited jobs resource

**Status:** `DONE`

**Why now**

The watchlist is a saved state and should not disappear because the jobs page is scoped to a narrower scan window.

**Primary files**

- `app/src/App.tsx`
- `app/src/views/WatchlistView.tsx`
- `app/src-tauri/src/lib.rs`
- `app/src-tauri/src/db/mod.rs`

**Exact tasks**

- [x] Add a dedicated backend query for watchlisted jobs.
- [x] Stop deriving watchlist content from the main `get_jobs(... daysAgo)` resource.
- [x] Keep applied/watchlist toggle behavior shared, but isolate the resource loading path.
- [x] Add a frontend test or integration harness that proves a saved watchlisted job remains visible regardless of the jobs date range.

**Acceptance criteria**

- [x] Watchlisted jobs remain visible when the scan date range changes.
- [x] Toggle actions still refresh the watchlist correctly.

**Suggested commit**

- `fix(ui): load watchlist from dedicated persisted data source`

### P1.7 - Tighten Ezer chat URL opening and conversation loading

**Status:** `DONE`

**Why now**

Ezer currently opens looser URLs than the rest of the app and can race conversation loads.

**Primary files**

- `app/src/views/EzerView.tsx`
- `app/src/views/JobsView.tsx`
- `app/src/views/WatchlistView.tsx`

**Exact tasks**

- [x] Extract one shared URL-opening helper used by Jobs, Watchlist, and Ezer.
- [x] Reuse the same `https` + allowlisted-host validation in Ezer.
- [x] Add request identity or cancellation guards to `loadMessages` so out-of-order responses do not overwrite the selected conversation.
- [x] Add UI tests for:
  - invalid AI card URL
  - fast conversation switching

**Acceptance criteria**

- [x] Ezer cannot open URLs that Jobs/Watchlist would reject.
- [x] Rapid chat switching does not show the wrong conversation.

**Suggested commit**

- `fix(ui): unify safe URL opening and guard Ezer conversation races`

---

## Phase 2 - Sidecar Architecture Decision

**Goal:** stop carrying two partial runtime architectures without an explicit boundary.

**Gate to exit Phase 2**

- A clear written decision exists: `KEEP SIDECAR` or `RETIRE SIDECAR`.
- The decision includes platform support expectations and CI impact.
- The next-phase task list is updated to match the chosen path.

### P2.1 - Inventory what the Python sidecar still does

**Status:** `DONE`

**Primary files**

- `ai_service/server.py`
- `app/src-tauri/src/ai/sentence_service.rs`
- `app/src-tauri/src/crawler/mod.rs`
- `app/src-tauri/src/ai_service_manager.rs`
- `README.md`

**Inventory**

| Responsibility | Current sidecar endpoint/code | Native replacement state | Decision |
|---|---|---|---|
| Embedding fallback | `POST /embed` in `ai_service/server.py`; HTTP fallback in `app/src-tauri/src/ai/sentence_service.rs` | Already replaced natively by `fastembed`/ONNX via `native_embedder`; Phase 1 also locked the model namespace to `all-MiniLM-L6-v2` end to end. | Retire HTTP fallback after adding one final native-embedding regression around error surfacing. |
| Resume text extraction fallback | `POST /extract-text` in `ai_service/server.py`; HTTP fallback in `sentence_service.rs` | Already replaced natively by `native_resume_parser` for `.pdf`, `.docx`, and `.txt`; Phase 1 narrowed the import boundary to exactly those extensions and copies files into app data before parsing. | Retire HTTP fallback. The sidecar supports extra extensions like `.md`/`.rtf`, but those are outside the accepted backend contract. |
| Scrapling search fallback | `POST /extract-search`; `Crawler::try_scrapling_search_fallback` and `try_scrapling_bruntwork_fallback` | Partially replaced but sufficient for production: OnlineJobs uses native static parsing; Bruntwork search has `try_parse_bruntwork_next_data` plus `parse_bruntwork_search_html`, both covered by parser tests. Current Bruntwork search pages expose listing content to non-headless crawlers/search indexing as of 2026-04-22. | Retire scrapling as a production dependency; keep native parser tests and add fixture coverage if a live Bruntwork regression appears. |
| Scrapling details fallback | `POST /extract-details`; `Crawler::try_scrapling_details_fallback` | Partially replaced by the Tauri WebView scraper in `fetch_job_details`, static `parse_bruntwork_job_details`, and `fetch_rsc_payload` + `try_parse_rsc_payload_description`. Current Bruntwork detail pages expose readable descriptions and published dates to non-headless crawlers/search indexing as of 2026-04-22. | Retire scrapling as a production dependency; the WebView/RSC/static chain is the supported fallback stack. |
| Health endpoint for diagnostics | `GET /health`; `ai_service_manager::snapshot` probes `http://127.0.0.1:8765/health` | Still sidecar-specific. It only diagnoses an optional fallback process and should not remain a primary settings concept once the sidecar is retired. | Replace with native runtime diagnostics for Ollama/native embedder/cache paths, then delete sidecar startup diagnostics. |

**Exact tasks**

- [x] Document every current sidecar responsibility:
  - embedding fallback
  - resume text extraction fallback
  - scrapling search/details fallback
  - health endpoint used by diagnostics
- [x] Mark each one as:
  - already replaced natively
  - partially replaced
  - still required
- [x] Verify whether Bruntwork scraping still needs scrapling after the current webview + RSC parsing path.

**Acceptance criteria**

- [x] There is a one-page inventory of remaining sidecar responsibilities inside this plan or an attached decision note.

### P2.2 - Decision checkpoint: keep or retire

**Status:** `DONE`

**Exact tasks**

- [x] Choose one path and record the rationale in this file under `Decision Log`.

**If KEEP SIDECAR**

- [x] Define supported platforms. `DROPPED`: sidecar will not be a supported production runtime.
- [x] Define packaging method for the sidecar and Python runtime. `DROPPED`: no packaging path will be added.
- [x] Remove `.venv/bin/uvicorn` assumptions from startup logic. `DROPPED`: delete startup logic during retirement instead.
- [x] Add CI coverage that exercises the packaged or supported startup flow. `DROPPED`: CI should cover native runtime paths instead.

**If RETIRE SIDECAR**

- [x] Prove Bruntwork search/details parity without scrapling.
- [x] Delete HTTP embedding and text-extraction fallback code.
- [x] Delete sidecar startup manager and sidecar docs once fallback parity is complete.

**Acceptance criteria**

- [x] There is no ambiguity left in code comments, docs, or CI about whether `ai_service` is a required production dependency.

**Suggested commit**

- `docs(architecture): record sidecar keep-or-retire decision`

---

## Phase 3 - Refactor After the Architecture Decision

**Goal:** simplify the codebase around the chosen runtime model and reduce coupling.

**Gate to exit Phase 3**

- `lib.rs` is no longer the de facto home for every backend concern.
- The frontend no longer uses one broad invalidation bus for all data.
- Shared contracts/configs are centralized.
- Duplicate business logic is removed or clearly delegated to one layer.

### P3.1 - Split backend command adapters from services

**Status:** `TODO`

**Primary files**

- `app/src-tauri/src/lib.rs`
- `app/src-tauri/src/ai/sentence_service.rs`
- `app/src-tauri/src/crawler/mod.rs`
- `app/src-tauri/src/ai_service_manager.rs`
- `ai_service/`
- new modules under `app/src-tauri/src/commands/`
- new modules under `app/src-tauri/src/services/`

**Exact tasks**

- [ ] Create thin command modules:
  - `commands/jobs.rs`
  - `commands/scan.rs`
  - `commands/ai.rs`
  - `commands/settings.rs`
- [ ] Create service modules:
  - [x] `services/scan_service.rs`
  - [ ] `services/ai_chat_service.rs` (`IN PROGRESS`: turn setup, routing, formatting, JSON response shapes, and semantic fallback extracted)
  - [x] `services/runtime_service.rs`
- [x] Retire sidecar-specific runtime code before extracting the runtime service:
  - remove HTTP `/embed` and `/extract-text` fallbacks from `sentence_service.rs`
  - remove scrapling HTTP fallbacks from `crawler/mod.rs`
  - delete `ai_service_manager.rs` and the `_ai_service` app state handle
  - delete or archive `ai_service/` after README/CI no longer reference it as active runtime
- [ ] Move business logic out of `lib.rs`; keep `lib.rs` focused on:
  - module wiring
  - `AppState`
  - Tauri plugin setup
  - handler registration
- [ ] Preserve existing command names to avoid frontend churn during the split.
- [ ] Add or update tests for moved service-layer functions before deleting old code.

**Acceptance criteria**

- [ ] `lib.rs` becomes a thin composition root.
- [ ] AI chat routing, scan orchestration, and runtime startup each live in dedicated modules.
- [x] No production code starts or probes the Python sidecar.

**Suggested commit sequence**

- `refactor(runtime): retire python sidecar fallbacks`
- `refactor(backend): extract scan service from lib.rs` (`DONE`)
- `refactor(backend): extract ai chat service from lib.rs` (`IN PROGRESS`)
- `refactor(backend): extract runtime startup service from lib.rs` (`DONE`)

### P3.2 - Replace broad frontend invalidation with explicit resources

**Status:** `TODO`

**Primary files**

- `app/src/App.tsx`
- `app/src/views/ScanView.tsx`
- `app/src/views/JobsView.tsx`
- `app/src/views/WatchlistView.tsx`
- `app/src/views/EzerView.tsx`

**Exact tasks**

- [ ] Replace the single `version()` bump pattern with separate refresh keys for:
  - jobs
  - runs
  - keywords
  - watchlist
  - resume/settings state
- [ ] Remove `setInterval`-based invalidation during scans.
- [ ] Keep scan progress driven by the Tauri channel, not polling.
- [ ] Ensure mutations refresh only the resources they actually affect.
- [ ] Reduce duplicate `refetchKeywords()` / `refetchRuns()` calls caused by the current shared invalidation model.

**Acceptance criteria**

- [ ] Watchlist/applied toggles no longer refetch unrelated resources.
- [ ] Scans do not rely on interval-driven resource bumps.
- [ ] Resource flow is understandable per feature.

**Suggested commit**

- `refactor(frontend): replace global version bump invalidation with feature-scoped refresh`

### P3.3 - Push more filtering and shaping into SQLite

**Status:** `TODO`

**Primary files**

- `app/src-tauri/src/db/mod.rs`
- `app/src-tauri/src/lib.rs`
- `app/src/views/JobsView.tsx`
- `app/src/views/WatchlistView.tsx`

**Exact tasks**

- [ ] Add dedicated backend query shapes for:
  - watchlisted jobs
  - latest-run jobs
  - optional source/schedule/pay filters where practical
- [ ] Move expensive, repeated client-side shaping out of the biggest tables first.
- [ ] Keep UI-only presentation transforms in the frontend, but move data filtering and selection to the backend.
- [ ] Add DB tests for the new query helpers.

**Acceptance criteria**

- [ ] The frontend no longer has to fetch all jobs just to render basic filtered views.
- [ ] Jobs and watchlist screens render from purpose-built result sets.

**Suggested commit**

- `refactor(data): move core job filtering paths into SQLite queries`

### P3.4 - Centralize pay normalization

**Status:** `TODO`

**Primary files**

- `app/src-tauri/src/db/mod.rs`
- `app/src/views/JobsView.tsx`
- `app/src/utils/`

**Exact tasks**

- [ ] Choose one layer as the source of truth for pay normalization. Recommended: backend.
- [ ] Expose normalized pay band or normalized hourly/monthly fields from the backend if the UI needs them.
- [ ] Remove the duplicated parsing heuristics from `JobsView.tsx`.
- [ ] Add regression tests for pay bucketing that match backend ranking assumptions.

**Acceptance criteria**

- [ ] UI filters and backend salary ranking use the same normalization contract.
- [ ] PHP/USD conversion and hourly/monthly heuristics do not diverge across layers.

**Suggested commit**

- `refactor(pay): centralize pay normalization in backend contract`

### P3.5 - Centralize runtime config and typed IPC contracts

**Status:** `TODO`

**Primary files**

- `app/src-tauri/src/ai/mod.rs`
- `app/src-tauri/src/crawler/mod.rs`
- `app/src/App.tsx`
- `app/src/components/SettingsPanel.tsx`
- new `app/src/types/ipc.ts` or generated contract output

**Exact tasks**

- [ ] Define one runtime-config source of truth for:
  - Ollama base URL
  - native embedding cache/model defaults
  - timeout defaults
  - embedding model contract
- [x] Remove sidecar/scrapling URL settings from active runtime config after retirement.
- [ ] Stop mixing DB-backed config and hardcoded localhost assumptions without a defined precedence.
- [ ] Create a real shared TS contract module for commonly reused shapes.
- [ ] Move duplicated interfaces out of `App.tsx`, `SettingsPanel.tsx`, `JobsView.tsx`, `WatchlistView.tsx`, and `EzerView.tsx`.
- [ ] If generation is feasible, document and adopt it; otherwise centralize the handwritten contract first.

**Acceptance criteria**

- [ ] Runtime defaults match across Rust and frontend.
- [ ] README no longer references a missing `ipc-contract.ts`.
- [ ] Shared TS shapes are imported from one module.

**Suggested commit**

- `refactor(contract): centralize runtime config and shared IPC types`

---

## Phase 4 - Operational Alignment

**Goal:** make CI, docs, and tests describe and enforce the architecture that actually exists.

**Gate to exit Phase 4**

- CI validates the real repo layout and runtime model.
- Docs match the current code.
- Integration coverage exists for the highest-risk flows.

### P4.1 - Rewrite CI to match the real repository

**Status:** `TODO`

**Primary files**

- `.github/workflows/ci.yml`
- `app/package.json`
- `app/src-tauri/Cargo.toml`

**Exact tasks**

- [ ] Remove stale root-level Python assumptions from CI.
- [ ] Delete Python CI work that no longer validates shipped behavior.
- [ ] Add explicit npm scripts for checks now hardcoded in CI, such as:
  - typecheck
  - frontend build smoke
  - backend lint/check wrappers if useful
- [ ] Consider platform matrix coverage if the chosen runtime architecture requires platform-specific startup validation.

**Acceptance criteria**

- [ ] CI validates the architecture the app actually ships.
- [ ] Local dev commands and CI commands match.

**Suggested commit**

- `ci: align workflow with actual app and sidecar architecture`

### P4.2 - Add regression coverage for orchestration boundaries

**Status:** `TODO`

**Primary files**

- `app/src/` test additions
- `app/src-tauri/tests/`

**Exact tasks**

- [ ] Add frontend integration tests for:
  - watchlist persistence independent of date range
  - Ezer conversation switching race guard
  - safe URL opening behavior
- [ ] Add backend tests for:
  - early URL allowlist rejection
  - embedding-model config enforcement
  - non-blocking startup diagnostics contract where practical
  - foreign key cascade cleanup
- [ ] Extend the existing eval harness only after the runtime contract is stable.

**Acceptance criteria**

- [ ] High-risk flows found in this review have regression coverage.
- [ ] New architectural boundaries have tests, not just comments.

**Suggested commit**

- `test: add regression coverage for startup, watchlist, Ezer, and URL safety`

### P4.3 - Repair docs and remove stale guidance

**Status:** `TODO`

**Primary files**

- `README.md`
- `app/README.md`
- `config/keywords.yaml`
- `TODO.md`

**Exact tasks**

- [ ] Update the root README architecture section to describe the chosen runtime architecture.
- [ ] Remove references to missing files such as `app/src/types/ipc-contract.ts`.
- [ ] Replace the stock `app/README.md` with project-specific guidance or delete it if redundant.
- [ ] Decide whether `config/keywords.yaml` is real configuration:
  - wire it into bootstrapping, or
  - remove its mention and possibly the file itself
- [ ] Reduce duplicate planning docs if they are superseded by this file and active TODOs.

**Acceptance criteria**

- [ ] A new contributor can follow the docs without running into stale architecture claims.
- [ ] There is one clear source of truth for active execution tracking.

**Suggested commit**

- `docs: align repository guidance with current architecture`

---

## Suggested Commit Sequence

Use this as the default slicing unless implementation details force a regroup:

1. `fix(ai): enforce a single supported embedding model contract`
2. `fix(security): validate job URLs before webview fallback`
3. `fix(security): harden resume import and limit resume data exposure`
4. `fix(runtime): make sidecar startup non-blocking and surface real diagnostics`
5. `fix(db): enable foreign keys and test cascade behavior`
6. `fix(ui): load watchlist from dedicated persisted data source`
7. `fix(ui): unify safe URL opening and guard Ezer conversation races`
8. `docs(architecture): record sidecar keep-or-retire decision`
9. `refactor(runtime): retire python sidecar fallbacks`
10. `refactor(backend): extract scan service from lib.rs`
11. `refactor(backend): extract ai chat service from lib.rs`
12. `refactor(backend): extract runtime startup service from lib.rs`
13. `refactor(frontend): replace global version bump invalidation with feature-scoped refresh`
14. `refactor(data): move core job filtering paths into SQLite queries`
15. `refactor(pay): centralize pay normalization in backend contract`
16. `refactor(contract): centralize runtime config and shared IPC types`
17. `ci: align workflow with actual app and sidecar architecture`
18. `test: add regression coverage for startup, watchlist, Ezer, and URL safety`
19. `docs: align repository guidance with current architecture`

---

## Decision Log

### 2026-04-22

- Full-codebase review completed after `HEAD` `970c1c1`.
- Phase order locked as:
  1. correctness/security
  2. sidecar decision
  3. refactor
  4. operational alignment
- Working assumption for planning:
  - native Rust remains the preferred path for embeddings and resume parsing
  - Python sidecar remains undecided until Bruntwork fallback needs are verified
- Architecture decision: `RETIRE SIDECAR`.
  - Production support expectation: the app supports the native Rust/Tauri runtime, SQLite, Ollama, native `fastembed` embeddings, native resume parsing, and Rust/WebView/RSC crawler paths. Python, FastAPI, scrapling, Playwright, and `.venv/bin/uvicorn` are no longer production runtime requirements.
  - CI impact: CI should validate `npm test`, `cargo test`, frontend build/type checks, and native parser/crawler coverage. It should not install Python sidecar dependencies unless a legacy/archival sidecar test job is explicitly added outside the production gate.
  - Rationale: the sidecar responsibilities are either already replaced natively or are fallback-only. Keeping the sidecar would require cross-platform Python packaging, browser binary management, and CI startup coverage for a path the product no longer needs as a first-class dependency.
  - Next action: begin Phase 3 with `refactor(runtime): retire python sidecar fallbacks` before splitting services, so the refactor does not preserve obsolete runtime boundaries.

---

## Tracking Notes

- Update each workstream status inline in this file.
- Add a dated bullet in `Decision Log` whenever scope changes.
- If a task is dropped, mark it `DROPPED` and record the reason instead of deleting it.
