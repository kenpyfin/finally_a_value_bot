#!/bin/sh
# Create vault Python venv with ChromaDB and embedding support.
set -e
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
WORKSPACE="${WORKSPACE_DIR:-${FINALLY_A_VALUE_BOT_WORKSPACE_DIR:-/app/workspace}}"
VENV_DIR="$WORKSPACE/shared/.venv-vault"

echo "Creating vault venv at $VENV_DIR"
python3 -m venv "$VENV_DIR"
"$VENV_DIR/bin/pip" install --quiet chromadb openai
echo "Done. Use: $VENV_DIR/bin/python $SCRIPT_DIR/query_vault.py \"query\""
