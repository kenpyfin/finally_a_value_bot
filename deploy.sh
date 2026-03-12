#!/usr/bin/env bash
set -euo pipefail

log() {
  printf '%s\n' "$*"
}

log "Deploying FinallyAValueBot..."

# Optional: git pull if run from a clone
if [ -d .git ]; then
  log "Pulling latest changes..."
  git pull --rebase || true
fi

log "Building and starting containers..."
docker compose build --no-cache
docker compose up -d

# Optional: start cursor-agent host runner if CURSOR_AGENT_RUNNER_URL is set
if [ -f .env ] && grep -q '^CURSOR_AGENT_RUNNER_URL=' .env 2>/dev/null; then
  url=$(grep '^CURSOR_AGENT_RUNNER_URL=' .env | cut -d= -f2- | tr -d '"' | tr -d "'")
  if [ -n "$url" ]; then
    port=$(echo "$url" | sed -n 's/.*:\([0-9]*\)$/\1/p')
    port=${port:-3847}
    if ! command -v python3 >/dev/null 2>&1; then
      log "Note: CURSOR_AGENT_RUNNER_URL is set but python3 not found; start the runner manually: python3 scripts/cursor-agent-runner.py $port"
    else
      # Always kill any existing runner so the new one inherits the current PATH
      pkill -f "cursor-agent-runner.py" 2>/dev/null && log "Killed old cursor-agent-runner." || true
      sleep 1
      log "Starting cursor-agent runner on port $port..."
      nohup python3 scripts/cursor-agent-runner.py "$port" > /dev/null 2>&1 &
      log "Runner started (PID $!)."
    fi
  fi
fi

log ""
log "FinallyAValueBot deployed."
log "Web UI: http://localhost:10961"
