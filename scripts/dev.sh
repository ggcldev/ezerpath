#!/usr/bin/env bash
# Launches the ezerpath desktop app. The scrapling fallback service is
# auto-detected at http://127.0.0.1:8765 (override with EZER_SCRAPLING_BASE_URL).
# Usage: ./scripts/dev.sh
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SCRAPLING_URL="${EZER_SCRAPLING_BASE_URL:-http://127.0.0.1:8765}"

if ! curl -sf "$SCRAPLING_URL/health" >/dev/null 2>&1; then
  echo "⚠️  Scrapling service not responding at $SCRAPLING_URL"
  echo "    Bruntwork descriptions won't load until you start it:"
  echo "      cd ai_service && source .venv/bin/activate && uvicorn server:app --host 127.0.0.1 --port 8765"
  echo ""
fi

cd "$REPO_ROOT/app"
exec npx tauri dev
