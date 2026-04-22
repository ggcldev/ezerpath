# AI + Scrapling Service

Legacy fallback only. As of 2026-04-22, Ezerpath's production runtime is the native Rust/Tauri path for embeddings, resume parsing, and crawler fallback behavior. This Python service remains temporarily for comparison/debugging while Phase 3 retires the sidecar code.

Local service used by the Tauri app for:
- Sentence-transformers embeddings (`/embed`)
- Resume file text extraction (`/extract-text`)
- **Headless JS rendering fallback** for sites like Bruntwork (`/extract-details`, `/extract-search`) — powered by [scrapling](https://github.com/D4Vinci/Scrapling) (Playwright).

## Run locally

```bash
cd ai_service
python3 -m venv .venv
source .venv/bin/activate
pip install -r requirements.txt
# First-time only: install Playwright browser binaries
python -m playwright install chromium
uvicorn server:app --host 127.0.0.1 --port 8765
```

Health check:

```bash
curl -s http://127.0.0.1:8765/health
```

## Connecting the Tauri app

The Rust crawler **auto-connects** to `http://127.0.0.1:8765` — no env vars or flags needed. If the service is running when you open a job, it'll be used automatically. If not, the crawler gracefully falls back to static HTML parsing.

Just run the app normally:

```bash
cd app && npx tauri dev
```

Override the URL (e.g. if you run scrapling on a different port) with:

```bash
export EZER_SCRAPLING_BASE_URL=http://127.0.0.1:9000
```
