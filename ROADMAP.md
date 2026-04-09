# Ezerpath — Product Overview & Roadmap

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

### AI-Powered

- [ ] Resume builder — generate a resume from user's skills/experience
- [ ] Tailored resume — feed a specific job description + user profile to Claude, get a custom resume
- [ ] Job match scoring — AI ranks how well a job matches the user's profile

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

### Phase 3 — AI Integration
- [ ] Claude API integration in Rust
- [ ] Resume builder — input form + AI generation
- [ ] Tailored resume per job — one-click generate
- [ ] Job match scoring

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
