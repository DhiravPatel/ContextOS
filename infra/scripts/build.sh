#!/usr/bin/env bash
# One-shot full-stack build: Rust release binary + compiled extension.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT"

echo "› building rust workspace"
cargo build --release --workspace

echo "› compiling vscode extension"
if [ ! -d apps/extension/node_modules ]; then
  (cd apps/extension && npm install --no-audit --no-fund)
fi
(cd apps/extension && npm run compile)

# Stage the CLI inside the extension so it ships bundled.
PLATFORM_DIR="apps/extension/bin/$(uname -s | tr '[:upper:]' '[:lower:]')"
mkdir -p "$PLATFORM_DIR"
cp target/release/contextos "$PLATFORM_DIR/contextos"

echo "✓ done. CLI: target/release/contextos; ext: apps/extension/out/extension.js"
