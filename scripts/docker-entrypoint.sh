#!/bin/sh
set -e
# Ensure vector DB directory exists for vault semantic search (ChromaDB)
mkdir -p /app/workspace/shared/vault_db

# Create vault venv (ChromaDB + openai) if setup script exists and venv is missing or broken
# Prefer built-in scripts/vault/setup_vault_env.sh; fall back to workspace/shared/
SETUP_SCRIPT=/app/scripts/vault/setup_vault_env.sh
if [ ! -f "$SETUP_SCRIPT" ]; then
  SETUP_SCRIPT=/app/workspace/shared/setup_vault_env.sh
fi
if [ -f "$SETUP_SCRIPT" ]; then
  NEED_SETUP=0
  VENV_DIR="${FINALLY_A_VALUE_BOT_WORKSPACE_DIR:-/app/workspace}/shared/.venv-vault"
  if [ ! -d "$VENV_DIR" ]; then
    NEED_SETUP=1
  elif ! "$VENV_DIR/bin/python" -c "import chromadb" 2>/dev/null; then
    NEED_SETUP=1
  fi
  if [ "$NEED_SETUP" = "1" ]; then
    echo "Setting up vault Python environment (chromadb)..."
    bash "$SETUP_SCRIPT" || true
  fi
fi

# Configure git so push works inside the container (bot runs git push via bash tool).
if [ -n "${GIT_USERNAME:-}" ] && [ -n "${GIT_TOKEN:-}" ]; then
  export HOME="${HOME:-/root}"
  git config --global credential.helper '!f() { echo "username=${GIT_USERNAME}"; echo "password=${GIT_TOKEN}"; }; f'
  # Embed credentials in the vault remote URL so push always works (no reliance on helper being invoked).
  VAULT_DIR="${FINALLY_A_VALUE_BOT_WORKSPACE_DIR:-/app/workspace}/${VAULT_ORIGIN_VAULT_PATH:-shared/ORIGIN}"
  REPO="${VAULT_ORIGIN_VAULT_REPO:-}"
  if [ -n "$REPO" ] && [ -d "$VAULT_DIR/.git" ]; then
    AUTH_URL="https://${GIT_USERNAME}:${GIT_TOKEN}@${REPO#https://}"
    if (cd "$VAULT_DIR" && git remote get-url origin >/dev/null 2>&1); then
      (cd "$VAULT_DIR" && git remote set-url origin "$AUTH_URL")
    fi
  fi
fi

exec "$@"
