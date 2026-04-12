from __future__ import annotations

from pathlib import Path
from typing import List

import numpy as np
from docx import Document
from fastapi import FastAPI, HTTPException
from pydantic import BaseModel, Field
from pypdf import PdfReader
from sentence_transformers import SentenceTransformer

app = FastAPI(title="Ezerpath AI Embedding Service", version="0.1.0")

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
