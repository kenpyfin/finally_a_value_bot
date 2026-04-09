---

name: search-vault
description: Semantically search the ORIGIN vault (Obsidian/markdown knowledge base) using vector similarity via ChromaDB.
license: MIT
compatibility:
  os:
    - darwin
    - linux
  deps:

- python3

---

# Search Vault

Use this skill when the user asks to search their knowledge base, vault, notes, or ORIGIN documents for information by meaning (semantic search).

## How it works

The `search_vault` tool is registered automatically when vault config is present. It uses a local ChromaDB vector database and an embedding server to perform semantic similarity search over indexed markdown documents.

### Prerequisites

A Python venv with `chromadb` and `openai` must exist. Run the bundled setup script to create it:

```bash
# From the skill directory:
bash skills/search-vault/setup_vault_env.sh
```

This creates `shared/.venv-vault/` with the required packages.

The canonical configuration source is the repository-root `.env` (or `FINALLY_A_VALUE_BOT_CONFIG` if set). Skill-local `.env` files are not used.

An embedding server must be running and accessible at `VAULT_EMBEDDING_SERVER_URL` (default: `http://127.0.0.1:8080`). Any OpenAI-compatible embedding API works (e.g. llama.cpp with `--embedding`).

### Environment variables


| Variable                     | Description                      | Default                 |
| ---------------------------- | -------------------------------- | ----------------------- |
| `VAULT_EMBEDDING_SERVER_URL` | Embedding API base URL           | `http://127.0.0.1:8080` |
| `VAULT_VECTOR_DB_PATH`       | ChromaDB persistent storage path | `shared/vault_db`       |
| `VAULT_VECTOR_DB_COLLECTION` | ChromaDB collection name         | `origin_vault`          |


### Usage

The `search_vault` tool is the primary interface. Just call it with a natural language query:

```
search_vault(query="machine learning concepts", n_results=5)
```

If the tool is not available, you can run the bundled script directly:

```bash
shared/.venv-vault/bin/python skills/search-vault/query_vault.py "search terms" [n_results]
```

### Troubleshooting

- **"No relevant results"**: The vault may not be indexed yet. Run the index-vault skill first.
- **Embedding server errors**: Verify the embedding server is running at the configured URL.
- **ChromaDB import errors**: Re-run `setup_vault_env.sh` to recreate the venv.

