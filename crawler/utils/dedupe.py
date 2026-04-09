#!/usr/bin/env python3
"""Standalone dedup utility — merge multiple JSONL files and remove duplicates.

Usage:
    python -m crawler.utils.dedupe

Reads all files in data/raw/, deduplicates by source:source_id,
and writes a merged file to data/snapshots/merged-YYYY-MM-DD.jsonl.
"""

import json
from datetime import datetime
from pathlib import Path


def dedupe():
    raw_dir = Path("data/raw")
    out_dir = Path("data/snapshots")
    out_dir.mkdir(parents=True, exist_ok=True)

    seen = {}
    for f in sorted(raw_dir.glob("jobs-*.jsonl")):
        for line in f.read_text(encoding="utf-8").splitlines():
            line = line.strip()
            if not line:
                continue
            row = json.loads(line)
            uid = f"{row.get('source', '')}:{row.get('source_id', '')}"
            # Keep the latest version of each job
            seen[uid] = row

    today = datetime.now().strftime("%Y-%m-%d")
    out = out_dir / f"merged-{today}.jsonl"
    with out.open("w", encoding="utf-8") as fh:
        for row in seen.values():
            fh.write(json.dumps(row, ensure_ascii=False) + "\n")

    print(f"Deduplicated {len(seen)} unique jobs -> {out}")


if __name__ == "__main__":
    dedupe()
