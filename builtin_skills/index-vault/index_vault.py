#!/usr/bin/env python3
"""Built-in vault index script. Index ORIGIN vault markdown files into ChromaDB for semantic search."""
import os
import re
import sys
from pathlib import Path

from dotenv import load_dotenv

SCRIPT_DIR = Path(__file__).parent.resolve()
load_dotenv(SCRIPT_DIR / ".env")


def _default_vault_db_path() -> str:
    root = os.environ.get("WORKSPACE_DIR") or os.environ.get("FINALLY_A_VALUE_BOT_WORKSPACE_DIR") or os.getcwd()
    return os.path.abspath(os.path.join(root, "shared", "vault_db"))


def _default_vault_path() -> str:
    root = os.environ.get("WORKSPACE_DIR") or os.environ.get("FINALLY_A_VALUE_BOT_WORKSPACE_DIR") or os.getcwd()
    rel = os.environ.get("VAULT_ORIGIN_VAULT_PATH", "shared/ORIGIN")
    return os.path.abspath(os.path.join(root, rel))


def _require_embed_openai_base() -> str:
    url = (os.environ.get("VAULT_EMBEDDING_SERVER_URL") or os.environ.get("VAULT_EMBED_URL") or "").strip()
    if not url:
        print(
            "error: VAULT_EMBEDDING_SERVER_URL or VAULT_EMBED_URL must be set (no default). "
            "Add `.env` beside this script or export the variable before running.",
            file=sys.stderr,
        )
        sys.exit(1)
    return url.rstrip("/") + "/v1" if not url.endswith("/v1") else url


DB_PATH = os.environ.get("VAULT_VECTOR_DB_PATH") or os.environ.get("VAULT_DB_PATH") or _default_vault_db_path()
print(f"DEBUG: DB_PATH={DB_PATH}")
VAULT_PATH = _default_vault_path()
EMBED_URL = _require_embed_openai_base()
COLLECTION = os.environ.get("VAULT_VECTOR_DB_COLLECTION", "origin_vault")

import chromadb
from chromadb.utils import embedding_functions

class CustomLlamaEF(chromadb.EmbeddingFunction):
    def __init__(self, embed_url: str):
        self.embed_url = embed_url.rstrip("/") + "/embeddings"

    def __call__(self, input: chromadb.Documents) -> chromadb.Embeddings:
        import requests
        payload = {
            "model": "ignored",
            "input": input
        }
        response = requests.post(self.embed_url, json=payload)
        response.raise_for_status()
        data = response.json()
        return [d["embedding"] for d in data["data"]]

llama_ef = CustomLlamaEF(embed_url=EMBED_URL)

client = chromadb.PersistentClient(path=DB_PATH)
collection = client.get_or_create_collection(name=COLLECTION, embedding_function=llama_ef)


def chunk_md(content: str, path: str, chunk_size: int = 2080, overlap: int = 400) -> list[tuple[str, str]]:
    """Split markdown into overlapping chunks."""
    content = re.sub(r"\s+", " ", content.strip())
    chunks = []
    start = 0
    while start < len(content):
        end = start + chunk_size
        chunk = content[start:end]
        if chunk.strip():
            chunks.append((chunk, path))
        start = end - overlap
    return chunks


def index_vault() -> int:
    vault = Path(VAULT_PATH)
    if not vault.exists():
        print(f"Vault path does not exist: {vault}", file=sys.stderr)
        return 1

    docs, metas, ids = [], [], []
    idx = 0
    for md in vault.rglob("*.md"):
        try:
            text = md.read_text(encoding="utf-8", errors="replace")
            rel = str(md.relative_to(vault))
            for chunk, _ in chunk_md(text, rel):
                docs.append(chunk)
                metas.append({"path": rel})
                ids.append(f"{rel}:{idx}")
                idx += 1
        except Exception as e:
            print(f"Skip {md}: {e}", file=sys.stderr)

    if not docs:
        print("No documents to index.")
        return 0

    print(f"Prepared {len(docs)} chunks. Starting upsert...")
    try:
        collection.upsert(documents=docs, metadatas=metas, ids=ids)
        print(f"Successfully indexed {len(docs)} chunks from {vault}")
    except Exception as e:
        print(f"Upsert failed: {e}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(index_vault())
