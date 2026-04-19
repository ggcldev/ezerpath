from __future__ import annotations

import re
from pathlib import Path
from typing import List, Optional

import numpy as np
from bs4 import BeautifulSoup
from docx import Document
from fastapi import FastAPI, HTTPException
from pydantic import BaseModel, Field
from pypdf import PdfReader
from sentence_transformers import SentenceTransformer

app = FastAPI(title="Ezerpath AI + Scrapling Service", version="0.2.0")

_model_cache: dict[str, SentenceTransformer] = {}


class EmbedRequest(BaseModel):
    texts: List[str] = Field(default_factory=list)
    model: str = "all-MiniLM-L6-v2"


class EmbedResponse(BaseModel):
    vectors: List[List[float]]
    model: str


class ExtractTextRequest(BaseModel):
    file_path: str


class ExtractTextResponse(BaseModel):
    text: str


def get_model(model_name: str) -> SentenceTransformer:
    if model_name not in _model_cache:
        _model_cache[model_name] = SentenceTransformer(model_name)
    return _model_cache[model_name]


def extract_text_from_pdf(path: Path) -> str:
    reader = PdfReader(str(path))
    chunks: List[str] = []
    for page in reader.pages:
        chunks.append(page.extract_text() or "")
    return "\n".join(chunks).strip()


def extract_text_from_docx(path: Path) -> str:
    doc = Document(str(path))
    return "\n".join(p.text for p in doc.paragraphs).strip()


def extract_text_from_txt(path: Path) -> str:
    return path.read_text(encoding="utf-8", errors="ignore").strip()


@app.get("/health")
def health() -> dict:
    return {"ok": True, "message": "Embedding service is running"}


@app.post("/embed", response_model=EmbedResponse)
def embed(payload: EmbedRequest) -> EmbedResponse:
    if not payload.texts:
        return EmbedResponse(vectors=[], model=payload.model)
    try:
        model = get_model(payload.model)
        arr = model.encode(payload.texts, convert_to_numpy=True, normalize_embeddings=True)
        vectors = np.asarray(arr, dtype=np.float32).tolist()
        return EmbedResponse(vectors=vectors, model=payload.model)
    except Exception as exc:
        raise HTTPException(status_code=500, detail=f"Embedding failed: {exc}") from exc


@app.post("/extract-text", response_model=ExtractTextResponse)
def extract_text(payload: ExtractTextRequest) -> ExtractTextResponse:
    path = Path(payload.file_path).expanduser().resolve()
    if not path.exists() or not path.is_file():
        raise HTTPException(status_code=404, detail="File not found")

    ext = path.suffix.lower()
    try:
        if ext in {".txt", ".md", ".rtf"}:
            text = extract_text_from_txt(path)
        elif ext == ".pdf":
            text = extract_text_from_pdf(path)
        elif ext == ".docx":
            text = extract_text_from_docx(path)
        else:
            raise HTTPException(status_code=400, detail=f"Unsupported file type: {ext}")
    except HTTPException:
        raise
    except Exception as exc:
        raise HTTPException(status_code=500, detail=f"Failed to extract text: {exc}") from exc

    if not text.strip():
        raise HTTPException(status_code=400, detail="No extractable text found in file")
    return ExtractTextResponse(text=text)


# ---------------------------------------------------------------------------
# Scrapling fallback: headless JS rendering for sites that need it (Bruntwork)
# ---------------------------------------------------------------------------

class ScraplingSearchRequest(BaseModel):
    url: str
    keyword: str = ""
    html: Optional[str] = None


class ScraplingJob(BaseModel):
    source_id: Optional[str] = None
    title: Optional[str] = None
    company: Optional[str] = None
    company_logo_url: Optional[str] = None
    pay: Optional[str] = None
    posted_at: Optional[str] = None
    url: Optional[str] = None
    summary: Optional[str] = None


class ScraplingSearchResponse(BaseModel):
    jobs: List[ScraplingJob] = Field(default_factory=list)


class ScraplingDetailsRequest(BaseModel):
    url: str
    html: Optional[str] = None


class ScraplingDetailsResponse(BaseModel):
    company: Optional[str] = None
    poster_name: Optional[str] = None
    company_logo_url: Optional[str] = None
    description: Optional[str] = None
    description_html: Optional[str] = None
    posted_at: Optional[str] = None


async def _fetch_rendered_html(url: str) -> str:
    """Fetch a URL with a real headless browser so client-side JS runs.
    Uses scrapling's StealthyFetcher async API (Playwright under the hood)."""
    from scrapling.fetchers import StealthyFetcher  # lazy import
    page = await StealthyFetcher.async_fetch(
        url,
        headless=True,
        network_idle=True,
        timeout=30000,
    )
    if page is None or not getattr(page, "html_content", None):
        raise HTTPException(status_code=502, detail="Rendered page was empty")
    return page.html_content


def _extract_bruntwork_details(soup: BeautifulSoup, url: str) -> ScraplingDetailsResponse:
    # Bruntwork renders the description in the main article region once JS runs.
    # Strategy: find the largest <article>, <main>, or <div> text block, excluding nav/footer.
    for tag_name in ("article", "main"):
        container = soup.find(tag_name)
        if container and len(container.get_text(strip=True)) > 200:
            break
    else:
        # Fall back to the largest div by text length
        container = max(
            (d for d in soup.find_all("div") if d.get_text(strip=True)),
            key=lambda d: len(d.get_text(strip=True)),
            default=None,
        )

    description_html = ""
    description = ""
    if container is not None:
        # Remove nav, footer, script, style, and "Apply Now" buttons
        for junk in container.find_all(["nav", "footer", "script", "style", "button", "header"]):
            junk.decompose()
        description_html = str(container)
        description = container.get_text("\n", strip=True)

    # Posted date: Bruntwork shows "Published on <date>"
    posted_at = ""
    body_text = soup.get_text("\n", strip=True)
    m = re.search(r"Published on\s*\n?\s*([A-Za-z]+ \d{1,2} \d{4})", body_text)
    if m:
        posted_at = m.group(1)

    return ScraplingDetailsResponse(
        company="BruntWork",
        poster_name="",
        company_logo_url="",
        description=description,
        description_html=description_html,
        posted_at=posted_at,
    )


def _extract_generic_details(soup: BeautifulSoup) -> ScraplingDetailsResponse:
    # Generic fallback: find the longest element matching typical description classes
    candidates = soup.select(
        "[class*='description'], [class*='job-body'], [class*='content'], article, main"
    )
    best = max(
        (c for c in candidates),
        key=lambda c: len(c.get_text(strip=True)),
        default=None,
    )
    if best is None:
        return ScraplingDetailsResponse()
    for junk in best.find_all(["script", "style", "nav", "footer"]):
        junk.decompose()
    return ScraplingDetailsResponse(
        description=best.get_text("\n", strip=True),
        description_html=str(best),
    )


@app.post("/extract-details", response_model=ScraplingDetailsResponse)
async def extract_details(req: ScraplingDetailsRequest) -> ScraplingDetailsResponse:
    try:
        html = req.html if req.html else await _fetch_rendered_html(req.url)
    except HTTPException:
        raise
    except Exception as exc:
        raise HTTPException(status_code=500, detail=f"Fetch failed: {exc}") from exc

    soup = BeautifulSoup(html, "html.parser")

    if "bruntworkcareers.co" in req.url:
        return _extract_bruntwork_details(soup, req.url)
    return _extract_generic_details(soup)


@app.post("/extract-search", response_model=ScraplingSearchResponse)
async def extract_search(req: ScraplingSearchRequest) -> ScraplingSearchResponse:
    # Minimal implementation — only used as a last-resort for search pages.
    # For Bruntwork, the main crawler already handles search via static HTML.
    try:
        html = req.html if req.html else await _fetch_rendered_html(req.url)
    except HTTPException:
        raise
    except Exception as exc:
        raise HTTPException(status_code=500, detail=f"Fetch failed: {exc}") from exc

    soup = BeautifulSoup(html, "html.parser")
    jobs: List[ScraplingJob] = []
    for a in soup.select("a[href*='/jobs/']"):
        href = a.get("href") or ""
        if not href.startswith("http"):
            href = "https://www.bruntworkcareers.co" + href
        title = a.get_text(strip=True)
        if not title or len(title) < 5:
            continue
        source_id = href.rstrip("/").split("/")[-1]
        jobs.append(ScraplingJob(
            source_id=source_id,
            title=title,
            url=href,
            company="BruntWork",
        ))
    # De-dupe by source_id
    seen = set()
    unique: List[ScraplingJob] = []
    for j in jobs:
        if j.source_id in seen:
            continue
        seen.add(j.source_id)
        unique.append(j)
    return ScraplingSearchResponse(jobs=unique)
