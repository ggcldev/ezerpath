# Ezerpath — Historical Planning (Archived 2026-04-22)

> **Archived.** Superseded by `README.md` and `AI_COPILOT_IMPLEMENTATION_PLAN.md`.
> This document references Claude API (actual implementation uses local Ollama)
> and mobile targets (iOS/Android) that are not on the current roadmap. Kept
> for historical context; do not treat as current truth.

## Vision

A lightweight, cross-platform job hunting app that crawls job boards, organizes listings with a dashboard, and uses AI to help users build tailored resumes and apply faster.

## Tech Stack

| Layer | Tech | Role |
|-------|------|------|
| App shell | Tauri 2 | Cross-platform desktop (Mac, Windows, Linux) + mobile (iOS, Android) |
| Frontend | SolidJS + TypeScript | Dashboard UI, watchlist, resume builder |
| Backend | Rust (inside Tauri) | Crawler engine, API calls, business logic |
| Database | SQLite | Local storage — jobs, watchlist, resumes, user prefs |
| AI | Claude API | Resume generation, job-tailored resumes, job matching/scoring |

## Architecture

```
┌──────────────────────────────────────┐
│  Tauri 2 (Rust)                      │
│  ┌────────────────────────────────┐  │
│  │  SolidJS + TypeScript          │  │
│  │  - Job feed / dashboard table  │  │
│  │  - Watchlist / saved jobs      │  │
│  │  - Resume builder              │  │
│  │  - Apply (webview to OLJ)      │  │
│  │  - Settings / keyword config   │  │
│  └──────────┬─────────────────────┘  │
│          Tauri IPC commands          │
│  ┌──────────▼─────────────────────┐  │
│  │  Rust Backend                  │  │
│  │  - Crawler (reqwest + scraper) │  │
│  │  - Claude AI integration       │  │
│  │  - SQLite (rusqlite)           │  │
│  │  - Job dedup + scheduling      │  │
│  └────────────────────────────────┘  │
└──────────────────────────────────────┘
```

## Features

### Core

- [ ] Job dashboard — table view of crawled listings (title, company, pay, date, keyword)
- [ ] Watchlist — save/bookmark jobs for later
- [ ] Multi-keyword search — configure keywords to track
- [ ] Date filtering — only show jobs from the last N days
- [ ] Dedup — track previously seen jobs, highlight new ones
- [ ] Auto-crawl scheduler — daily scan at a set time
- [ ] Background new-job monitor + desktop notifications — detect newly posted jobs and show OS-level toast/notification even when app window is minimized (macOS + Windows)

### AI-Powered

- [ ] Resume builder — generate a resume from user's skills/experience
- [ ] Tailored resume — feed a specific job description + user profile to Claude, get a custom resume
- [ ] Job match scoring — AI ranks how well a job matches the user's profile
- [ ] AI chat copilot — ask natural-language questions about scanned jobs
- [ ] Resume upload + AI matching — upload CV/resume and rank best-fit jobs from current scan
- [ ] Smart rescan recommendations — if matches are weak or empty, AI suggests next best keywords and scan strategy
- [ ] Job summarization — concise summaries for selected jobs and grouped summaries for multiple jobs

### Account & Apply

- [ ] OnlineJobs.ph login — webview inside the app to log in
- [ ] Apply directly — open the job application page within the app
- [ ] Application tracker — log which jobs the user applied to and status

### Platform

- [ ] macOS desktop
- [ ] Windows desktop
- [ ] iOS mobile
- [ ] Android mobile

## Phases

### Phase 1 — Foundation (Current)
- [x] Scrapy crawler prototype (Python — proof of concept)
- [x] Multi-keyword crawling with date filter
- [x] Dedup pipeline
- [x] Markdown table reports
- [ ] Initialize Tauri 2 + SolidJS project
- [ ] Rewrite crawler in Rust (reqwest + scraper)
- [ ] SQLite storage for jobs
- [ ] Basic dashboard UI — job table with search/filter

### Phase 2 — Dashboard & Watchlist
- [ ] Watchlist — save/unsave jobs
- [ ] Job detail view
- [ ] Keyword management UI
- [ ] Crawl-on-demand button
- [ ] Auto-crawl scheduler (background task)
- [ ] Filter/sort by keyword, date, pay, new/seen
- [ ] Minimized app notifier — run background scan checks and fire "New job posted" notifications with quick-open action

### Phase 2.2 — Crawler Resilience Fallback
- [ ] Keep Rust crawler as primary path (`reqwest + scraper`)
- [ ] Add retry/backoff tuning and selector health metrics per board
- [ ] Add optional Scrapling sidecar fallback for hard pages only (not full backbone)
- [ ] Add sidecar health check + fail-fast behavior so failed fallback does not block scans
- [ ] Add strict output schema validation before writing fallback results to SQLite
- [ ] Add settings toggle and runtime flags for enabling/disabling fallback safely
- [ ] Add logs to track fallback usage rate and extraction success/failure
- [ ] Add acceptance tests for: primary success, primary fail + fallback success, fallback unavailable

### Phase 2.1 — Background Monitoring & Notifications (Detailed Plan)
- [ ] Notification permission flow (desktop)
  - [ ] Add first-run permission prompt for notifications
  - [ ] Add settings toggle: `Enable desktop notifications`
  - [ ] Add settings toggle: `Notify only for new jobs (not updated jobs)`
- [ ] Background monitor service (Rust)
  - [ ] Create monitor loop that runs while app is open/minimized
  - [ ] Add configurable polling interval (default: every 5 minutes)
  - [ ] Respect crawl lock to avoid overlapping scans
  - [ ] Exponential backoff on crawl/network failures
- [ ] New-job detection logic (SQLite)
  - [ ] Store `last_notified_at` per job or per scan run
  - [ ] Notify only once per unique job id
  - [ ] Track `new_since_last_check` count for badge/toast summary
- [ ] Notification delivery (Tauri desktop)
  - [ ] Integrate Tauri notification API for macOS/Windows toasts
  - [ ] Notification payload: title, company, keyword, posted time
  - [ ] Add click action: open app + focus `All Jobs` + highlight latest new jobs
  - [ ] Add action button (if supported): `Open Job`
- [ ] UI integration (SolidJS)
  - [ ] Add monitor status indicator (idle/checking/error) in sidebar
  - [ ] Add unread new-jobs badge near `All Jobs`
  - [ ] Add settings panel for interval and notification preferences
- [ ] Noise control / anti-spam
  - [ ] Bundle multiple new jobs into one summary toast when count > threshold
  - [ ] Cooldown window to prevent repeated alerts (ex: max 1 summary / 2 min)
  - [ ] Quiet hours option (optional, phase 2.2)
- [ ] Platform-specific QA
  - [ ] Test minimized-window notifications on macOS
  - [ ] Test minimized-window notifications on Windows
  - [ ] Validate behavior when app regains focus after notification click
- [ ] Telemetry/logging (local-only)
  - [ ] Log monitor runs, detected counts, notification dispatch outcome
  - [ ] Add debug view/export for troubleshooting notification failures
- [ ] Acceptance criteria
  - [ ] User receives desktop toast for newly detected jobs while app is minimized
  - [ ] Clicking toast reliably opens app and lands on relevant new jobs context
  - [ ] No duplicate notifications for the same job
  - [ ] Monitor loop stays lightweight and does not block normal UI usage

### Phase 3 — AI Integration
- [ ] Claude API integration in Rust
- [ ] Resume builder — input form + AI generation
- [ ] Tailored resume per job — one-click generate
- [ ] Job match scoring

### Phase 3.1 — AI Copilot for Job Discovery (Detailed Plan)
- [ ] AI chat panel in app (UI)
  - [ ] Add persistent chat drawer/window with conversation history per user
  - [ ] Add quick prompts: `Best jobs for me`, `Summarize selected job`, `Suggest keywords`
  - [ ] Add context chips so user can include/exclude filters (date range, keyword, watchlist only)
- [ ] Resume ingestion pipeline
  - [ ] Add resume upload (`.pdf`, `.docx`, `.txt`)
  - [ ] Extract text locally (Rust pipeline) and normalize into structured profile
  - [ ] Store parsed profile in SQLite with versioning (so user can update resume over time)
- [ ] Job-to-resume matching engine
  - [ ] Build baseline relevance scoring (skills overlap, role/title similarity, compensation fit)
  - [ ] Add AI reranking step for top-N jobs to improve precision
  - [ ] Return explainable match reasons (why this job is a fit, gaps, caveats)
- [ ] “No strong matches” fallback intelligence
  - [ ] Detect low-confidence results or zero relevant jobs
  - [ ] Generate recommended scan keywords from resume + prior scans
  - [ ] Offer one-click `Start New Scan` using suggested keywords and date range
- [ ] Job summarization capabilities
  - [ ] Single-job summary: role, requirements, compensation, risks, action items
  - [ ] Multi-job comparison summary: top options, tradeoffs, who should apply
  - [ ] Copy/export summary to clipboard for quick sharing
- [ ] Prompting + context orchestration
  - [ ] Build context builder from local DB (`jobs`, `watchlist`, `keywords`, `resume_profile`)
  - [ ] Add token-budget strategy (truncate/segment long descriptions safely)
  - [ ] Add prompt templates for Q&A, matching, keyword recommendation, summarization
- [ ] Safety, privacy, and controls
  - [ ] Add explicit user consent before sending resume/job text to AI API
  - [ ] Add settings: model/provider, max tokens, temperature, request timeout
  - [ ] Add redact mode for sensitive resume sections before API call
- [ ] Reliability and UX guardrails
  - [ ] Add graceful fallback when AI API is unavailable
  - [ ] Add loading states, partial streaming output, and retry actions
  - [ ] Cache recent AI outputs per job/resume hash to reduce repeated cost/latency
- [ ] Observability (local-first)
  - [ ] Log AI request metadata locally (latency, token counts, success/failure)
  - [ ] Add debug view for prompt/context preview (developer mode)
- [ ] Acceptance criteria
  - [ ] User can ask chat questions about available jobs and get grounded answers from scanned data
  - [ ] User can upload a resume and receive ranked best-fit jobs with explanations
  - [ ] When matches are weak, user receives actionable keyword suggestions and can launch a new scan in one click
  - [ ] User can generate readable summaries for one job or multiple jobs without leaving the app

### Phase 4 — Account & Apply
- [ ] OnlineJobs.ph login via webview
- [ ] In-app job application flow
- [ ] Application tracker

### Phase 5 — Mobile
- [ ] Tauri 2 mobile targets (iOS + Android)
- [ ] Responsive SolidJS UI
- [ ] Mobile-specific UX adjustments

## Supported Job Boards

| Board | Status | Notes |
|-------|--------|-------|
| OnlineJobs.ph | Active | Primary target, respects robots.txt + 5s crawl delay |
| Upwork | Placeholder | Requires approved API — do not scrape |
| (Future boards) | Planned | Architecture supports adding new board adapters |

## Design Principles

- **Lightweight** — single binary, no runtime dependencies, small memory footprint
- **Privacy-first** — all data stored locally in SQLite, no cloud sync required
- **Respectful crawling** — obey robots.txt, rate limits, and ToS
- **AI as assistant** — AI helps with resumes and matching, user stays in control
