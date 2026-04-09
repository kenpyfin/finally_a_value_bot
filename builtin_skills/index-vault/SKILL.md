---

name: index-vault
description: Index ORIGIN vault markdown files into ChromaDB for semantic search.
license: MIT
compatibility:
  os:
    - darwin
    - linux
  deps:
- python3

---

# Index Vault

Use this skill to index (or re-index) the ORIGIN vault into the ChromaDB vector database. This must be run before `search_vault` will return results, and should be re-run whenever vault content changes.

## How it works

The bundled `index_vault.py` script walks the ORIGIN vault directory for `*.md` files, chunks them into overlapping segments, generates embeddings via the embedding server, and upserts them into a local ChromaDB collection.

### Prerequisites

Same as the search-vault skill — a Python venv with `chromadb` and `openai`:

```bash
bash skills/index-vault/setup_vault_env.sh
```

An embedding server must be running at `VAULT_EMBEDDING_SERVER_URL`.

### Environment variables


| Variable                     | Description                                      | Default                 |
| ---------------------------- | ------------------------------------------------ | ----------------------- |
| `VAULT_ORIGIN_VAULT_PATH`    | Path to the ORIGIN vault (relative to workspace) | `shared/ORIGIN`         |
| `VAULT_EMBEDDING_SERVER_URL` | Embedding API base URL                           | `http://127.0.0.1:8080` |
| `VAULT_VECTOR_DB_PATH`       | ChromaDB persistent storage path                 | `shared/vault_db`       |
| `VAULT_VECTOR_DB_COLLECTION` | ChromaDB collection name                         | `origin_vault`          |


### Usage

Run the indexing script directly:

```bash
shared/.venv-vault/bin/python skills/index-vault/index_vault.py
```

Or if the system Python has chromadb installed:

```bash
python3 skills/index-vault/index_vault.py
```

### Scheduling

Indexing is automatically scheduled to run every 6 hours. The agent receives a prompt to run this script and report the result. You can also trigger it manually at any time by asking the agent to re-index the vault.

### Chunking strategy

- Chunk size: 1000 characters
- Overlap: 200 characters
- Whitespace is normalized before chunking
- Each chunk is stored with its source file path as metadata

