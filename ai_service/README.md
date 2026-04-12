# AI Embedding Service

Local service used by the Tauri app for:
- Sentence-transformers embeddings (`/embed`)
- Resume file text extraction (`/extract-text`)

## Run locally

```bash
cd ai_service
python3 -m venv .venv
source .venv/bin/activate
pip install -r requirements.txt
uvicorn server:app --host 127.0.0.1 --port 8765
```

Health check:

```bash
curl -s http://127.0.0.1:8765/health
```
