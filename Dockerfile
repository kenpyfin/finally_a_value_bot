# Multi-stage build for FinallyAValueBot with agent-browser support
# Stage 1: Build finally-a-value-bot binary (Rust) and web assets
FROM rust:1-bookworm AS builder

# Install Node.js for web build
RUN apt-get update && apt-get install -y --no-install-recommends \
    nodejs npm \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Build web assets first (required for cargo build - include_dir embeds web/dist)
COPY web/package.json web/package-lock.json* ./web/
RUN npm --prefix web ci
COPY web/ ./web/
RUN npm --prefix web run build

# Build Rust binary
COPY Cargo.toml Cargo.lock build.rs ./
COPY src/ ./src/
COPY builtin_skills/ ./builtin_skills/
RUN cargo build --release

# Stage 2: Runtime with Playwright (Chromium) + agent-browser
FROM mcr.microsoft.com/playwright:v1.48.0-noble AS runtime

# Install Node.js (playwright image may have it; ensure npm for agent-browser), git for vault push,
# and Python 3.12 for vault tools (query_vault, index_vault with ChromaDB)
# Retry apt-get on hash mismatch (transient Ubuntu CDN issues)
RUN for i in 1 2 3; do apt-get update && apt-get install -y --no-install-recommends \
    curl ca-certificates git python3.12 python3.12-venv \
    && rm -rf /var/lib/apt/lists/* && break || sleep 15; done

# Install agent-browser globally (browser tool uses this path in container)
RUN npm install -g agent-browser && agent-browser install
ENV AGENT_BROWSER_PATH=/usr/local/bin/agent-browser

# Copy finally-a-value-bot binary from builder
COPY --from=builder /build/target/release/finally-a-value-bot /usr/local/bin/finally-a-value-bot

# Built-in vault scripts (query_vault, index_vault)
COPY scripts/vault/ /app/scripts/vault/

# Entrypoint ensures workspace layout (e.g. shared/vault_db for vector DB)
COPY scripts/docker-entrypoint.sh /app/scripts/docker-entrypoint.sh
RUN chmod +x /app/scripts/docker-entrypoint.sh

WORKDIR /app

# Default config path (override with volume)
ENV FINALLY_A_VALUE_BOT_CONFIG=/app/finally-a-value-bot.config.yaml

# Web UI port
EXPOSE 10961

# Chromium needs more shared memory; 1gb recommended
ENV NODE_OPTIONS="--max-old-space-size=512"

ENTRYPOINT ["/app/scripts/docker-entrypoint.sh"]
CMD ["finally-a-value-bot", "start"]
