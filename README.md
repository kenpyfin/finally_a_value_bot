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

### 1. Installation

Run the install script to download the latest binary:

```bash
curl -fsSL https://raw.githubusercontent.com/finally-a-value-bot/finally-a-value-bot/main/install.sh | bash
```

### 2. Configure

Run the interactive setup wizard — it covers **every** configuration option:

```bash
finally-a-value-bot setup
```

Or manually copy and edit the `.env` file:

```bash
cp .env.example .env
# Edit .env with your tokens and preferences
```

**Required** (at minimum one channel):
- `TELEGRAM_BOT_TOKEN` — from [@BotFather](https://t.me/BotFather)
- `BOT_USERNAME` — your bot's username (without @)
- `LLM_PROVIDER` — e.g. `anthropic`, `google`, `openai`, `ollama`
- `LLM_API_KEY` — your LLM API key

See `.env.example` for all available options (LLM, Discord, WhatsApp, Web UI, Vault, etc.).

### 3. Start

**Foreground:**
```bash
finally-a-value-bot start
```

**Background (Systemd/Launchd):**
The bot includes a built-in gateway manager to handle persistent background execution.
```bash
finally-a-value-bot gateway install   # Install and start service
finally-a-value-bot gateway status    # View status
finally-a-value-bot gateway logs      # View logs
```
On first run, FinallyAValueBot will send an onboarding message to get you started.

## Documentation

- [DOCKER.md](DOCKER.md): Running FinallyAValueBot in Docker.
- [ARCHITECTURE.md](ARCHITECTURE.md): Deep dive into the agentic loop and project structure.
- [DEVELOP.md](DEVELOP.md): How to contribute and build from source.
- [TEST.md](TEST.md): Testing guide.

## License

MIT
