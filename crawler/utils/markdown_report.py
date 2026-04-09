#!/usr/bin/env python3
"""Read the latest JSONL crawl output and produce a Markdown report."""

import json
import sys
from datetime import datetime
from pathlib import Path


def generate_report():
    raw_dir = Path("data/raw")
    report_dir = Path("reports")
    report_dir.mkdir(parents=True, exist_ok=True)

    jsonl_files = sorted(raw_dir.glob("jobs-*.jsonl"))
    if not jsonl_files:
        print("No JSONL files found in data/raw/", file=sys.stderr)
        sys.exit(1)

    latest = jsonl_files[-1]
    rows = []
    for line in latest.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if line:
            rows.append(json.loads(line))

    if not rows:
        print(f"No jobs in {latest.name}", file=sys.stderr)
        sys.exit(1)

    today = datetime.now().strftime("%Y-%m-%d")
    new_jobs = [r for r in rows if r.get("is_new")]
    seen_jobs = [r for r in rows if not r.get("is_new")]

    # Group by keyword
    by_keyword = {}
    for r in rows:
        kw = r.get("keyword", "unknown")
        by_keyword.setdefault(kw, []).append(r)

    lines = [
        f"# Daily Job Scan - {today}",
        "",
        f"- **Source file**: `{latest.name}`",
        f"- **Total jobs found**: {len(rows)}",
        f"- **New jobs**: {len(new_jobs)}",
        f"- **Previously seen**: {len(seen_jobs)}",
        f"- **Keywords searched**: {', '.join(by_keyword.keys())}",
        "",
    ]

    # New jobs table
    if new_jobs:
        lines.append("## New Jobs")
        lines.append("")
        lines.append("| # | Title | Company | Pay | Keyword | Posted | URL |")
        lines.append("|---|-------|---------|-----|---------|--------|-----|")
        for i, r in enumerate(new_jobs, 1):
            title = r.get("title", "No title").replace("|", "/")
            company = r.get("company", "") or "-"
            pay = r.get("pay", "") or "-"
            keyword = r.get("keyword", "")
            posted = r.get("posted_at", "")[:10] or "-"
            url = r.get("url", "")
            link = f"[View]({url})" if url else "-"
            lines.append(f"| {i} | {title} | {company} | {pay} | {keyword} | {posted} | {link} |")
        lines.append("")

    # Previously seen jobs table
    if seen_jobs:
        lines.append("## Previously Seen Jobs")
        lines.append("")
        lines.append("| # | Title | Keyword | URL |")
        lines.append("|---|-------|---------|-----|")
        for i, r in enumerate(seen_jobs, 1):
            title = r.get("title", "No title").replace("|", "/")
            keyword = r.get("keyword", "")
            url = r.get("url", "")
            link = f"[View]({url})" if url else "-"
            lines.append(f"| {i} | {title} | {keyword} | {link} |")
        lines.append("")

    out = report_dir / f"jobs-{today}.md"
    out.write_text("\n".join(lines), encoding="utf-8")
    print(f"Report written to {out}")


if __name__ == "__main__":
    generate_report()
