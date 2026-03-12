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

### 2. Setup

Navigate to your workspace directory and run:

```bash
finally-a-value-bot setup
```
This will guide you through configuring your Telegram bot, LLM provider, and workspace paths.

### 3. Start

```bash
finally-a-value-bot start
```
On first run, FinallyAValueBot will send an onboarding message to get you started.

## Documentation

- [DOCKER.md](DOCKER.md): Running FinallyAValueBot in Docker.
- [ARCHITECTURE.md](ARCHITECTURE.md): Deep dive into the agentic loop and project structure.
- [DEVELOP.md](DEVELOP.md): How to contribute and build from source.
- [TEST.md](TEST.md): Testing guide.

## License

MIT
