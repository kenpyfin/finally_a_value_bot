#!/usr/bin/env bash
set -euo pipefail

log() {
  printf '%s\n' "$*"
}

log "Deploying MicroClaw (dev: incremental build, no cache invalidation)..."

log "Building and starting containers..."
docker compose build
docker compose up -d

# Optional: start cursor-agent host runner if CURSOR_AGENT_RUNNER_URL is set
if [ -f .env ] && grep -q '^CURSOR_AGENT_RUNNER_URL=' .env 2>/dev/null; then
  url=$(grep '^CURSOR_AGENT_RUNNER_URL=' .env | cut -d= -f2- | tr -d '"' | tr -d "'")
  if [ -n "$url" ]; then
    port=$(echo "$url" | sed -n 's/.*:\([0-9]*\)$/\1/p')
    port=${port:-3847}
    if ! command -v python3 >/dev/null 2>&1; then
      log "Note: CURSOR_AGENT_RUNNER_URL is set but python3 not found; start the runner manually: python3 scripts/cursor-agent-runner.py $port"
    elif ! nc -z 127.0.0.1 "$port" 2>/dev/null; then
      log "Starting cursor-agent runner on port $port..."
      python3 scripts/cursor-agent-runner.py "$port" &
      log "Runner started (PID $!)."
    else
      log "Cursor-agent runner already running on port $port."
    fi
  fi
fi

log ""
log "MicroClaw deployed."
log "Web UI: http://localhost:10961"
