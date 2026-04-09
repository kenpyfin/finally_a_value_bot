#!/usr/bin/env python3
"""Built-in vault index script. Index ORIGIN vault markdown files into ChromaDB for semantic search."""
import os
import re
import sys
from pathlib import Path

from dotenv import load_dotenv

ROOT_DIR = Path(__file__).resolve().parents[2]
_CONFIG_PATH = os.environ.get("FINALLY_A_VALUE_BOT_CONFIG")
load_dotenv(Path(_CONFIG_PATH) if _CONFIG_PATH else ROOT_DIR / ".env")


def _default_vault_db_path() -> str:
    root = os.environ.get("WORKSPACE_DIR") or os.environ.get("FINALLY_A_VALUE_BOT_WORKSPACE_DIR") or os.getcwd()
    return os.path.abspath(os.path.join(root, "shared", "vault_db"))


def _default_vault_path() -> str:
    root = os.environ.get("WORKSPACE_DIR") or os.environ.get("FINALLY_A_VALUE_BOT_WORKSPACE_DIR") or os.getcwd()
    rel = os.environ.get("VAULT_ORIGIN_VAULT_PATH", "shared/ORIGIN")
    return os.path.abspath(os.path.join(root, rel))


def _default_embed_url() -> str:
    url = os.environ.get("VAULT_EMBEDDING_SERVER_URL") or os.environ.get("VAULT_EMBED_URL", "")
    if url:
        return url.rstrip("/") + "/v1" if not url.endswith("/v1") else url
    return "http://127.0.0.1:8080/v1"


DB_PATH = os.environ.get("VAULT_VECTOR_DB_PATH") or os.environ.get("VAULT_DB_PATH") or _default_vault_db_path()
VAULT_PATH = _default_vault_path()
EMBED_URL = _default_embed_url()
COLLECTION = os.environ.get("VAULT_VECTOR_DB_COLLECTION", "origin_vault")

import chromadb
from chromadb.utils import embedding_functions

llama_ef = embedding_functions.OpenAIEmbeddingFunction(
    api_key="sk-no-key-required",
    api_base=EMBED_URL,
    model_name="ignored",
)

client = chromadb.PersistentClient(path=DB_PATH)
collection = client.get_or_create_collection(name=COLLECTION, embedding_function=llama_ef)


def chunk_md(content: str, path: str, chunk_size: int = 1000, overlap: int = 200) -> list[tuple[str, str]]:
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

    collection.upsert(documents=docs, metadatas=metas, ids=ids)
    print(f"Indexed {len(docs)} chunks from {vault}")
    return 0


if __name__ == "__main__":
    sys.exit(index_vault())
