#!/usr/bin/env bash
# Refresh script: build the bot and restart the gateway so it runs the new code.
# Run this after adding features to ensure the gateway service picks them up.
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$ROOT_DIR"

BIN_NAME="finally-a-value-bot"
BIN="${BIN:-$ROOT_DIR/target/release/$BIN_NAME}"

# Same install-dir logic as install.sh so refresh updates the same place install put the binary.
detect_install_dir() {
  if [ -n "${FINALLY_A_VALUE_BOT_INSTALL_DIR:-}" ]; then
    echo "$FINALLY_A_VALUE_BOT_INSTALL_DIR"
    return
  fi
  if [ -w "/usr/local/bin" ]; then
    echo "/usr/local/bin"
    return
  fi
  if [ -d "$HOME/.local/bin" ] || mkdir -p "$HOME/.local/bin" 2>/dev/null; then
    echo "$HOME/.local/bin"
    return
  fi
  echo "/usr/local/bin"
}

usage() {
  cat <<'EOF'
Usage:
  ./refresh.sh [--help]

Builds the release binary, installs it to the same directory install.sh uses
(FINALLY_A_VALUE_BOT_INSTALL_DIR, or /usr/local/bin if writable, or ~/.local/bin), then
restarts the gateway. After this, "finally-a-value-bot start" runs the new code.

Override: BIN=/path/to/finally-a-value-bot ./refresh.sh  or  FINALLY_A_VALUE_BOT_INSTALL_DIR=/path ./refresh.sh
EOF
}

case "${1:-}" in
  -h|--help|help) usage; exit 0 ;;
esac

echo "== Building release binary =="
cargo build --release

if [ ! -x "$BIN" ]; then
  echo "Binary not found or not executable: $BIN" >&2
  exit 1
fi

echo ""
echo "== Installing to same location as install.sh =="
INSTALL_DIR="$(detect_install_dir)"
INSTALL_PATH="$INSTALL_DIR/$BIN_NAME"
USE_SUDO=""
if [ ! -w "$INSTALL_DIR" ] && command -v sudo >/dev/null 2>&1; then
  USE_SUDO="sudo"
fi

if [ "$(uname -s)" = "Darwin" ]; then
  # On macOS, a copied binary in ~/.local/bin can be killed when run (path-based security).
  # Use a symlink so 'finally-a-value-bot' runs the binary at repo path, which works.
  $USE_SUDO rm -f "$INSTALL_PATH"
  $USE_SUDO ln -sf "$ROOT_DIR/target/release/$BIN_NAME" "$INSTALL_PATH"
  echo "Linked $INSTALL_PATH -> $ROOT_DIR/target/release/$BIN_NAME"
else
  if [ -w "$INSTALL_DIR" ]; then
    cp "$BIN" "$INSTALL_PATH"
    echo "Updated $INSTALL_PATH"
  else
    if [ -n "$USE_SUDO" ]; then
      sudo cp "$BIN" "$INSTALL_PATH"
      echo "Updated $INSTALL_PATH (via sudo)"
    else
      echo "No write permission for $INSTALL_DIR and sudo not available. Run: sudo cp $BIN $INSTALL_PATH"
      exit 1
    fi
  fi
fi

echo ""
echo "== Restarting gateway =="
"$BIN" gateway stop 2>/dev/null || true
if "$BIN" gateway start; then
  echo "Gateway restarted (running new binary)."
else
  echo ""
  echo "Tip: if the gateway is not installed, run: $BIN gateway install"
  exit 1
fi

echo ""
echo "Done."
echo ""
echo "Note: The gateway is now running the new binary. On macOS, 'finally-a-value-bot' in PATH is a"
echo "      symlink to the repo binary so it runs correctly. You do not need 'finally-a-value-bot start'"
echo "      unless you want the bot in the foreground; then run: finally-a-value-bot gateway stop first."