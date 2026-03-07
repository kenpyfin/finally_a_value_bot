#!/bin/sh
# Cursor-agent host runner. Runs on the host so the bot (in Docker) can delegate cursor-agent execution.
# Requires: cursor-agent CLI and tmux installed on the host. Start this before the bot in Docker.
# Usage: ./cursor-agent-runner.sh [port]
# Default port: 3847
# Set CURSOR_AGENT_RUNNER_URL=http://host.docker.internal:3847 in .env when the bot runs in Docker.

set -e

PORT="${1:-3847}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
RUNNER_PY="$SCRIPT_DIR/cursor-agent-runner.py"

# Delegate to Python script (stdin-friendly for JSON)
exec python3 "$RUNNER_PY" "$PORT"
