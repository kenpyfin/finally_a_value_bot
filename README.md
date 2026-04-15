# FinallyAValueBot

FinallyAValueBot is a powerful agentic assistant that lives in your chat (Telegram, Discord, WhatsApp) and has direct access to your local workspace, tools, and Obsidian vault.

## Key Features

- **Agentic Loop**: Autonomous reasoning using LLMs (Claude, GPT, etc.).
- **Local Workspace**: Access to your files, bash, and browser.
- **Obsidian Integration**: Uses your vault as a knowledge base with semantic search.
- **Skill System**: Extensible with custom instructions and tools.
- **Multi-Channel**: Syncs conversations across Telegram, Discord, WhatsApp, and Web.
- **Self-Indexing**: Automatically schedules vault indexing and updates.

## Quick Start

### 1. Install the binary

```bash
curl -fsSL https://raw.githubusercontent.com/finally-a-value-bot/finally-a-value-bot/main/install.sh | bash
```

Alternatives: **Homebrew** (macOS) — `brew tap everettjf/tap && brew install finally-a-value-bot`; **Windows** — see [`install.ps1`](install.ps1) in this repo. Building from source is described in [DEVELOP.md](DEVELOP.md).

The bot reads configuration from a **`.env` file in the current working directory** (or from the path in `FINALLY_A_VALUE_BOT_CONFIG`). Use a dedicated project directory and keep your `.env` there.

### 2. Bootstrap `.env`

Copy the example file and set only bootstrap variables:

```bash
cp .env.example .env
# Set workspace + web bootstrap values in .env
```

Bootstrap variables stay in `.env` (repo root): workspace path, config path override, and web host/port/auth values needed before the web server starts.

### 3. Start and finish onboarding in Web UI

```bash
finally-a-value-bot start
```

Then open **http://127.0.0.1:10961**. Configure **LLM and bot tokens in repo-root `.env`** (see `.env.example`). Use **Settings** for bot integrations (extra instances), persona scope per channel, and optional **restart** when `FINALLY_A_VALUE_BOT_RESTART_COMMAND` is set. Changes to `.env` require a process restart.

**Useful checks:**

```bash
finally-a-value-bot doctor      # preflight diagnostics
finally-a-value-bot test-llm    # test LLM connectivity
finally-a-value-bot help        # all commands
```

**Minimum configuration**

- **Channel:** at least one of **Telegram** (`TELEGRAM_BOT_TOKEN` + `BOT_USERNAME`) or **Discord** (`DISCORD_BOT_TOKEN`).
- **LLM:** `LLM_PROVIDER` and `LLM_API_KEY` — except for local providers (`ollama`, `llama`, `llamacpp`) where the API key may be omitted.

See [.env.example](.env.example) for the full list (web UI, scheduler, vault, safety, etc.).

### 4. Web UI

When web is enabled (default), the UI is served at **http://127.0.0.1:10961** by default (`WEB_PORT`). If you bind to a non-local address, set `WEB_AUTH_TOKEN`.

### 5. Background service (optional)

```bash
finally-a-value-bot gateway install   # install and start service
finally-a-value-bot gateway status
finally-a-value-bot gateway logs
```

When channel + LLM configuration is complete, FinallyAValueBot seeds an onboarding task so you can get started from chat.

### Vault search (optional)

Built-in vault skills need an embedding endpoint. Configure `VAULT_*` in `.env` when you use the vault integration, and set `VAULT_EMBEDDING_SERVER_URL` in each skill’s local `.env` or in the process environment — see comments in [.env.example](.env.example).

## Documentation

- [ARCHITECTURE.md](ARCHITECTURE.md): Agentic loop and project structure.
- [DEVELOP.md](DEVELOP.md): Contributing and building from source.
- [TEST.md](TEST.md): Testing guide.
- [DOCKER.md](DOCKER.md): Legacy container notes — **not recommended** for new deployments; prefer the native binary and `gateway` above.

## License

MIT
