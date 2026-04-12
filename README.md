# Ezerpath

A local-first desktop job-hunting copilot. Crawls job boards, stores everything in a local SQLite database, and ships with **Ezer**, an on-device AI assistant that ranks jobs, summarizes listings, suggests keywords, and matches your resume — all running on your own machine via [Ollama](https://ollama.com).

> Built for people who want a private, fast, batteries-included alternative to web-based job dashboards. Nothing leaves your laptop.

---

## Stack

| Layer | Technology |
|---|---|
| Desktop shell | [Tauri 2](https://tauri.app) (Rust core + WebView UI) |
| Frontend | [SolidJS](https://www.solidjs.com) + TypeScript + [Tailwind CSS 4](https://tailwindcss.com) + Vite 6 |
| UI niceties | `lucide-solid`, `motion`, `number-flow`, `solid-toast` |
| Native backend | Rust (`tokio`, `reqwest`, `scraper`, `serde`, `chrono`) |
| Storage | SQLite via `rusqlite` (bundled, no system dep) |
| Crawler | In-process Rust crawler using `scraper` + `reqwest` |
| LLM runtime | [Ollama](https://ollama.com) — default model `qwen2.5:7b-instruct` |
| Embeddings + file extraction | Python FastAPI sidecar (`sentence-transformers`, `pypdf`, `python-docx`) |
| Tests | `vitest` (frontend), `cargo test` + `tempfile` (backend) |

---

## Architecture

```
┌──────────────────────────────────────────────────────────────────────────────┐
│  Tauri 2 desktop window  (1100×700, overlay title bar)                       │
│                                                                              │
│  ┌────────────────────────────────────────────────────────────────────────┐  │
│  │  SolidJS + Tailwind 4   (app/src)                                      │  │
│  │                                                                        │  │
│  │  ┌─────────┐  ┌──────────┐  ┌──────────────┐  ┌──────────┐             │  │
│  │  │ScanView │  │ JobsView │  │WatchlistView │  │ EzerView │  views/     │  │
│  │  └─────────┘  └──────────┘  └──────────────┘  └────┬─────┘             │  │
│  │                                                    │                   │  │
│  │  Sidebar · SettingsPanel · JobDetailsDrawer ·      │  components/      │  │
│  │  ConfirmModal · AnimatedNumber                     │                   │  │
│  │                                                    │                   │  │
│  │  utils/  jobs.ts (scope filters) · mutations.ts (try/catch wrap)       │  │
│  │          confirmations.ts · viewMotion.ts · fluidHover.ts              │  │
│  │                                                                        │  │
│  │  types/ipc-contract.ts   ◄── single source of truth, mirrors Rust      │  │
│  └────────────────────────────────┬───────────────────────────────────────┘  │
│                                   │  @tauri-apps/api  invoke()               │
│                                   ▼                                          │
│  ┌────────────────────────────────────────────────────────────────────────┐  │
│  │  Tauri IPC bridge ── 32 #[tauri::command] entry points (sync request/  │  │
│  │  response; no event channels — scans block on the awaited promise)    │  │
│  └────────────────────────────────┬───────────────────────────────────────┘  │
│                                   ▼                                          │
│  ┌────────────────────────────────────────────────────────────────────────┐  │
│  │  Rust core   (app/src-tauri/src/lib.rs)                                │  │
│  │                                                                        │  │
│  │  ┌──────────────────────────────────────────────────────────────────┐  │  │
│  │  │  AppState   (tauri-managed, shared by every command)             │  │  │
│  │  │    ├─ db:               Arc<Database>                            │  │  │
│  │  │    ├─ crawler:          Crawler                                  │  │  │
│  │  │    ├─ ollama:           OllamaClient                             │  │  │
│  │  │    ├─ sentence_service: SentenceServiceClient                    │  │  │
│  │  │    └─ crawl_lock:       Mutex<()>   ← rejects concurrent scans   │  │  │
│  │  └──────────────────────────────────────────────────────────────────┘  │  │
│  │                                                                        │  │
│  │   ai_chat command flow                                                 │  │
│  │     ┌─ classify_intent(message, recent_history)                        │  │
│  │     │                                                                  │  │
│  │     ├─► Ranking   ─► db::get_top_paying_jobs   (SQL-first, no LLM)     │  │
│  │     ├─► FollowUp  ─► resolve previously linked job ids → Ollama        │  │
│  │     ├─► Describe  ─► use cached summaries if any → else Ollama         │  │
│  │     └─► General   ─► OllamaClient::chat (streaming)                    │  │
│  │                                                                        │  │
│  │   Modules                                                              │  │
│  │     ai/                                  crawler/         db/         │  │
│  │      ├─ ollama.rs        stream NDJSON   └─ mod.rs        └─ mod.rs   │  │
│  │      │                   + idle-gap         fetch +          schema + │  │
│  │      │                   timeout            scraper +        queries +│  │
│  │      ├─ prompts.rs       system prompts     resilience       migrations│  │
│  │      ├─ ranking.rs       cosine_similarity  fallback                  │  │
│  │      └─ sentence_                                                     │  │
│  │         service.rs       HTTP client → ai_service                     │  │
│  └─────────┬────────────────┬─────────────────┬─────────────────────────────┘
└────────────┼────────────────┼─────────────────┼─────────────────────────────┘
             │                │                 │
             ▼                ▼                 ▼
   ┌──────────────────┐  ┌──────────┐  ┌──────────────────────────┐
   │ Ollama           │  │ Job      │  │ ai_service  (FastAPI)    │
   │ 127.0.0.1:11434  │  │ boards   │  │ 127.0.0.1:8765           │
   │  POST /api/chat  │  │ HTTPS    │  │  POST /embed             │
   │   (stream=true)  │  │          │  │  POST /extract-text      │
   │  GET  /api/tags  │  │          │  │  GET  /health            │
   └──────────────────┘  └──────────┘  └─────────────┬────────────┘
                                                     │
                                                     ▼
                                        ┌──────────────────────────┐
                                        │ sentence-transformers    │
                                        │ all-MiniLM-L6-v2         │
                                        │ + pypdf + python-docx    │
                                        └──────────────────────────┘

   ┌────────────────────────────────────────────────────────────────────┐
   │ Local SQLite — ezerpath.db                                         │
   │ (~/Library/Application Support/com.genylgicalde.ezerpath/ on macOS)│
   │                                                                    │
   │  jobs · keywords · runs                  ── crawl state            │
   │  resume_profiles                         ── uploaded resumes       │
   │  job_embeddings · resume_embeddings      ── vector cache (per model)│
   │  ai_conversations · ai_messages          ── chat history           │
   │  ai_runs                                 ── AI telemetry           │
   │  app_settings                            ── runtime config         │
   └────────────────────────────────────────────────────────────────────┘
```

A few honest notes about this picture:

- **Scans are synchronous.** A `crawl_jobs` invocation holds the `crawl_lock` and only resolves after every keyword has been crawled. The frontend awaits one promise — there are no progress events. If you want a live progress bar, that's a future change.
- **Two embedding tables, not one.** Jobs and resumes embed separately, both keyed by `(id, model_name)`, so you can swap embedding models without losing the others' cache.
- **`ranking.rs` is tiny.** It's just `cosine_similarity`. The "SQL-first ranking" isn't a Rust ranking module — it's the `lib.rs` intent-router branch that asks SQLite to sort by normalized salary, bypassing Ollama entirely.
- **No frontend query/cache layer.** `app/src/utils/` has small helpers (scope filters, a `runMutation` try/catch wrapper, motion easings) — not a TanStack-style cache. Views call `invoke()` directly and re-fetch on demand.
- **Nothing leaves localhost.** The only outbound traffic is the crawler hitting job boards. Ollama, the embedding sidecar, and SQLite are all on `127.0.0.1`.

### Repository layout

```
ezerpath/
├── app/                       # Tauri desktop app
│   ├── src/                   # SolidJS frontend
│   │   ├── views/             # ScanView, JobsView, WatchlistView, EzerView
│   │   ├── components/        # Sidebar, SettingsPanel, JobDetailsDrawer, …
│   │   ├── types/             # ipc-contract.ts (mirrors Rust structs)
│   │   └── utils/             # jobs, mutations, confirmations, viewMotion, fluidHover
│   ├── src-tauri/             # Rust backend
│   │   ├── src/
│   │   │   ├── lib.rs         # All #[tauri::command] entry points + AppState
│   │   │   ├── ai/
│   │   │   │   ├── ollama.rs  # Streaming Ollama chat client
│   │   │   │   ├── prompts.rs # System prompts
│   │   │   │   ├── ranking.rs # SQL-first job ranking
│   │   │   │   └── sentence_service.rs # Embedding service HTTP client
│   │   │   ├── crawler/mod.rs # Job board crawler
│   │   │   └── db/mod.rs      # SQLite schema + queries
│   │   ├── Cargo.toml
│   │   └── tauri.conf.json
│   └── package.json
├── ai_service/                # Python FastAPI sidecar
│   ├── server.py              # /embed, /extract-text, /health
│   └── requirements.txt
├── config/keywords.yaml       # Crawler keyword config
├── data/                      # Crawl snapshots, raw HTML cache
├── reports/                   # Generated job reports
└── README.md                  # ← you are here
```

---

## Features

- **Crawl** — built-in Rust crawler scrapes target job boards into local SQLite. Resilient fallback path if a request fails.
- **Browse & filter** — JobsView with keyword filter, watchlist filter, recency filter.
- **Watchlist** — star jobs you want to keep an eye on.
- **Ezer AI Copilot** (`EzerView`):
  - Chat with full conversation history persisted to SQLite.
  - **Intent router** — ranking / follow-up / describe / general are routed to SQL-first or Ollama paths automatically.
  - **Streaming chat** — NDJSON streaming with idle-gap timeout, so cold model loads and long completions never hit a wall-clock limit.
  - **Job cards** — assistant replies attach inline cards for the jobs they reference.
  - **Linked job IDs** — follow-up questions ("describe the second one") resolve against the previously cited jobs.
- **Resume matching** — upload a PDF/DOCX/TXT resume, the Python sidecar extracts text and produces embeddings, and Rust ranks jobs by cosine similarity.
- **Keyword suggestions** — Ollama-generated keyword ideas for your next scan.
- **Settings panel** — live edit Ollama URL, model, temperature, max tokens, request timeout, and embedding service URL.

---

## Running from scratch

### Prerequisites

| Tool | Version | Why |
|---|---|---|
| Rust toolchain | stable (1.77+) | Tauri core |
| Node.js | 18+ | Vite / Tauri CLI |
| Python | 3.10+ | Embedding sidecar |
| Ollama | latest | Local LLM runtime |
| Xcode CLT (macOS) / `build-essential` + `libwebkit2gtk-4.1-dev` (Linux) / WebView2 (Windows) | — | Tauri's webview |

Install Rust: <https://rustup.rs>
Install Ollama: <https://ollama.com/download>

### 1. Clone

```bash
git clone https://github.com/ggcldev/ezerpath.git
cd ezerpath
```

### 2. Pull a model into Ollama

```bash
ollama pull qwen2.5:7b-instruct       # default
# or any chat model you prefer; set it in Settings later
```

Start the Ollama server (it usually auto-starts on install):

```bash
ollama serve
```

Verify: `curl http://127.0.0.1:11434/api/tags`

### 3. Set up the Python embedding sidecar

```bash
cd ai_service
python3 -m venv .venv
source .venv/bin/activate         # Windows: .venv\Scripts\activate
pip install -r requirements.txt
uvicorn server:app --host 127.0.0.1 --port 8765
```

Leave it running. Verify: `curl http://127.0.0.1:8765/health`

> The first call to `/embed` downloads the `all-MiniLM-L6-v2` model (~80 MB). This is a one-time cost.

### 4. Install frontend deps

In a new terminal:

```bash
cd app
npm install
```

### 5. Run the desktop app

```bash
npx tauri dev
```

First build of the Rust crate takes a few minutes. After that, hot-reload kicks in for both the SolidJS frontend and the Rust backend.

The app window will open at `1100×700`. Open **Settings** in the sidebar to confirm:

- Ollama URL: `http://127.0.0.1:11434`
- Ollama model: `qwen2.5:7b-instruct` (or whatever you pulled)
- Embedding service URL: `http://127.0.0.1:8765`
- Timeout (ms): `120000` (default — bounds *idle gaps* between streamed tokens, not total generation time)

### 6. Build a release binary

```bash
cd app
npx tauri build
```

The signed bundle lands in `app/src-tauri/target/release/bundle/`.

---

## Configuration

| Setting | Default | Notes |
|---|---|---|
| `ollama_base_url` | `http://127.0.0.1:11434` | Any Ollama-compatible endpoint works |
| `ollama_model` | `qwen2.5:7b-instruct` | Use any model you've pulled |
| `embedding_service_url` | `http://127.0.0.1:8765` | The Python sidecar |
| `embedding_model` | `all-MiniLM-L6-v2` | Sentence-transformers model |
| `temperature` | `0.2` | Low for deterministic ranking output |
| `max_tokens` | `1024` | Per-reply generation cap |
| `timeout_ms` | `120_000` | **Idle-gap budget**, not total time |

All settings live in the SQLite DB and are editable from the in-app **Settings** panel.

---

## How a chat request flows

```
EzerView (SolidJS)
   │
   │  invoke('ai_chat', { message, conversation_id, … })
   ▼
lib.rs::ai_chat            (Rust, async)
   │
   ├── persist user message    → db::append_ai_message
   ├── load filtered jobs      → db::get_jobs
   │
   ├── classify_intent(message, recent_history)
   │     │
   │     ├── Ranking      → SQL-first via db::get_top_paying_jobs
   │     ├── FollowUp     → resolve previously linked job ids
   │     ├── Describe     → use cached job summaries if present
   │     └── General      → fall through
   │
   ├── If a fast path matched: format reply locally, return.
   │
   └── Otherwise: build system prompt + history
         │
         ▼
       OllamaClient::chat(cfg, messages)
         │  POST /api/chat  { stream: true }
         │  loop: read NDJSON chunk under idle-gap timeout
         │  accumulate message.content until { done: true }
         ▼
       Reply text → persisted → returned to frontend as AiChatResponse
```

The streaming client uses `tokio::time::timeout` per `Response::chunk()` call, so the only thing that can ever trip the timeout is genuine silence between tokens — not slow generation, not cold model loads.

---

## Database

SQLite file lives in the OS app-data dir (`~/Library/Application Support/com.genylgicalde.ezerpath/` on macOS). Tables include:

- `jobs` — crawled listings (`watchlisted` is a column on this table, not a separate table)
- `keywords` — seed list of search terms used by the crawler
- `runs` — crawl run history with `status`, `error_message`, `finished_at`, totals
- `resume_profiles` — uploaded resumes + extracted/normalized text
- `job_embeddings` — vector cache keyed by `(job_id, model_name)`
- `resume_embeddings` — vector cache keyed by `(resume_id, model_name)`
- `ai_conversations`, `ai_messages` — chat history (`ai_messages` carries `meta_json` and `linked_job_ids_json`)
- `ai_runs` — telemetry for AI calls (task type, latency, status, error)
- `app_settings` — runtime config key/value store

Schema and migrations live in `app/src-tauri/src/db/mod.rs`.

---

## IPC commands

The Rust core exposes 32 `#[tauri::command]` entry points covering jobs, watchlist, crawler runs, AI chat, embeddings, resume profiles, and settings. They're mirrored as TypeScript types in `app/src/types/ipc-contract.ts` so the frontend gets full autocompletion.

---

## Development

```bash
# Frontend tests
cd app && npm test

# Rust tests
cd app/src-tauri && cargo test

# Type-check frontend without running
cd app && npx tsc --noEmit

# Format Rust
cd app/src-tauri && cargo fmt
```

### Useful scripts

| From | Command | Does |
|---|---|---|
| `app/` | `npx tauri dev` | Full dev loop (Vite + cargo run + window) |
| `app/` | `npx tauri build` | Release bundle |
| `app/` | `npm run dev` | Vite only (no Rust window) |
| `ai_service/` | `uvicorn server:app --reload --port 8765` | Sidecar with reload |

---

## Troubleshooting

**"Ollama request timed out before completion."**
Make sure `ollama serve` is running and the selected model is pulled (`ollama list`). The default 120s idle-gap budget is generous, but if your hardware is very slow on first-token latency you can raise it in Settings.

**"Embedding service unreachable."**
Confirm `uvicorn` is running on port 8765 and that no firewall is blocking localhost. `curl http://127.0.0.1:8765/health` should return `{"ok": true, ...}`.

**Port 1420 already in use** when running `npx tauri dev`
A previous Vite process is still alive. `lsof -ti:1420 | xargs kill -9` and retry.

**First build is slow**
The Rust dependency tree compiles once. Subsequent builds are incremental.

---

## License

MIT — see `app/package.json`.
