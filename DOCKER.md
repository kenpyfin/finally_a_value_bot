# Docker Deployment

MicroClaw can run in Docker with agent-browser (headless Chromium) support.

## Quick Start

1. Copy and edit config:
   ```bash
   cp .env.example .env
   # Edit .env with your tokens, API keys, etc. (or run microclaw setup)
   ```

2. Ensure the workspace directory exists (default `./workspace`). The container entrypoint creates `workspace/shared/vault_db` for the vector DB on first run:
   ```bash
   mkdir -p workspace
   ```

3. Deploy:
   ```bash
   ./deploy.sh
   ```

4. Open Web UI: http://localhost:10961

## Config for Docker

When running in Docker, set these in `.env` if needed:

- **WORKSPACE_DIR**: Not required; compose sets `MICROCLAW_WORKSPACE_DIR=/app/workspace` (host `./workspace` is mounted there).
- **AGENT_BROWSER_PATH**: `/usr/local/bin/agent-browser` (override host path)
- **WEB_HOST**: `0.0.0.0` (to accept connections from host)
- **WEB_AUTH_TOKEN**: Required when web_host is not localhost
- **GIT_USERNAME** / **GIT_TOKEN**: Optional. When set, the container configures git credentials so `git push` (e.g. for the ORIGIN vault) works. Use your GitHub username and a [Personal Access Token](https://github.com/settings/tokens) (PAT) with **repo** scope (and push access to the vault repo). If you get 403, check the token is valid and has push permission.

## Volumes

- `./.env` → `/app/.env` (read-only)
- `./workspace` → `/app/workspace` (workspace: runtime, skills, shared, AGENTS.md)

## ORIGIN Vault and Vector DB

To use the ORIGIN Obsidian vault and vault semantic search:

1. Configure vault vars in `.env`: `VAULT_ORIGIN_VAULT_PATH=shared/ORIGIN`, `VAULT_VECTOR_DB_PATH=shared/vault_db`. See `.env.example`.
2. The container entrypoint ensures `workspace/shared/vault_db` exists and creates the vault Python venv (`.venv-vault` with ChromaDB) at startup if missing. Populate the index by running the index-vault skill after adding vault content.
3. Clone the vault into `workspace/shared/ORIGIN` or use the sync service.
4. **Optional sync from git**: set `VAULT_ORIGIN_VAULT_REPO` in `.env`, then run:
   ```bash
   docker compose --profile sync up -d
   ```
   The sync service reads `.env` and pulls into `workspace/shared/ORIGIN` every 15 minutes.

## Agent-Browser

The image includes agent-browser (npm) and Chromium (Playwright). Browser automation runs in headless mode only; no GUI. The `shm_size: "1gb"` setting is required for Chromium stability. The image sets `AGENT_BROWSER_PATH=/usr/local/bin/agent-browser` so the browser tool finds it. If you see "browser function is not installed" or "command not found", rebuild the image: `docker compose build --no-cache` (or `./deploy.sh`) so the `npm install -g agent-browser && agent-browser install` step runs again.

## Cursor Agent in Docker

The image does **not** install cursor-agent or tmux. cursor-agent is designed for desktop use with Cursor IDE auth and is typically not available in headless containers.

**Options:**

1. **Fallback (no setup)**: When cursor-agent is unavailable, `build_skill` falls back to direct file creation — it writes SKILL.md from the provided name, description, and instructions. Skill creation works in Docker without cursor-agent.

2. **Host runner**: To run cursor-agent from Docker, use a host runner. Install cursor-agent and tmux on the **host**, run the reference runner (see `scripts/cursor-agent-runner.sh`), and set `CURSOR_AGENT_RUNNER_URL` in `.env` (e.g. `http://host.docker.internal:3847`). The bot will POST spawn requests to the runner, which executes cursor-agent on the host. On Linux, `extra_hosts: host.docker.internal:host-gateway` in docker-compose allows the container to reach the host (already configured).

## Refresh / Update

To pull latest code and redeploy (full rebuild):

```bash
./deploy.sh
```

Or manually:

```bash
docker compose build --no-cache
docker compose up -d
```

## Development: Fast Refresh

For development, use `deploy-dev.sh` to rebuild incrementally (uses Docker cache):

```bash
./deploy-dev.sh
```

This skips `git pull` and `--no-cache`, so builds are much faster when only some layers changed. Use `./deploy.sh` for production or when you want a clean rebuild.

