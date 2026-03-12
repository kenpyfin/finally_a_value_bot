#!/usr/bin/env python3
"""Built-in vault query script. Semantically search the ORIGIN vault via ChromaDB."""
import os
import sys
from pathlib import Path

from dotenv import load_dotenv

SCRIPT_DIR = Path(__file__).parent.resolve()
load_dotenv(SCRIPT_DIR / ".env")
# Also load from workspace .env if present (when run from workspace dir)
load_dotenv(Path.cwd() / ".env")

def _default_vault_db_path() -> str:
    root = os.environ.get("WORKSPACE_DIR") or os.environ.get("FINALLY_A_VALUE_BOT_WORKSPACE_DIR") or os.getcwd()
    return os.path.abspath(os.path.join(root, "shared", "vault_db"))

def _default_embed_url() -> str:
    url = os.environ.get("VAULT_EMBEDDING_SERVER_URL") or os.environ.get("VAULT_EMBED_URL", "")
    if url:
        return url.rstrip("/") + "/v1" if not url.endswith("/v1") else url
    return "http://127.0.0.1:8080/v1"

DB_PATH = os.environ.get("VAULT_VECTOR_DB_PATH") or os.environ.get("VAULT_DB_PATH") or _default_vault_db_path()
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


def query(text: str, n: int = 5) -> None:
    try:
        results = collection.query(query_texts=[text], n_results=n)
        if not results["documents"] or not results["documents"][0]:
            print("No relevant results found.")
            return
        for i, doc in enumerate(results["documents"][0]):
            meta = results["metadatas"][0][i] if results["metadatas"] else {}
            path = meta.get("path", meta.get("source", "unknown"))
            excerpt = doc[:500] + "..." if len(doc) > 500 else doc
            print(f"--- Result {i + 1} (Source: {path}) ---")
            print(excerpt)
            print()
    except Exception as e:
        print(f"Query error: {e}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    if len(sys.argv) > 1:
        n = int(sys.argv[2]) if len(sys.argv) > 2 else 5
        query(sys.argv[1], n=n)
    else:
        print("Usage: python3 query_vault.py \"search terms\" [n_results]")
