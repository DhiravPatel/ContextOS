#!/usr/bin/env bash
#
# ContextOS terminal installer.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/DhiravPatel/ContextOS/main/infra/scripts/install.sh | bash
#
# That single command:
#   1. Detects OS + architecture.
#   2. Downloads the matching prebuilt `contextos` binary from the latest
#      GitHub Release.
#   3. Verifies the SHA-256 against the SHA256SUMS file in the same release.
#   4. Drops the binary into ~/.local/bin (or $CONTEXTOS_INSTALL_DIR).
#   5. If you ran the curl from inside a project directory, runs
#      `contextos init` to build the graph and wire Claude Code — no
#      follow-up commands needed.
#
# Environment overrides:
#   CONTEXTOS_VERSION=v0.2.1   # pin to a specific release tag (default: latest)
#   CONTEXTOS_INSTALL_DIR=...  # destination directory (default: ~/.local/bin)
#   CONTEXTOS_REPO=owner/repo  # different fork (default: DhiravPatel/ContextOS)
#   CONTEXTOS_INIT_DIR=path    # explicit project directory to init (default: $PWD if it's a project)
#   CONTEXTOS_NO_INIT=1        # install binary only, skip auto-init
#   CONTEXTOS_SKIP_BUILD=1     # auto-init but skip the graph build (faster, lazy build later)

set -euo pipefail

REPO="${CONTEXTOS_REPO:-DhiravPatel/ContextOS}"
VERSION="${CONTEXTOS_VERSION:-latest}"
INSTALL_DIR="${CONTEXTOS_INSTALL_DIR:-$HOME/.local/bin}"

# ---- helpers ---------------------------------------------------------------

err() { printf '\033[31merror:\033[0m %s\n' "$*" >&2; exit 1; }
info() { printf '\033[36m::\033[0m %s\n' "$*"; }
ok() { printf '\033[32m✓\033[0m %s\n' "$*"; }

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || err "missing required command: $1"
}

require_cmd curl
require_cmd uname
require_cmd tar
# shasum (macOS) or sha256sum (Linux); we accept either.
if command -v sha256sum >/dev/null 2>&1; then
  SHACMD="sha256sum"
elif command -v shasum >/dev/null 2>&1; then
  SHACMD="shasum -a 256"
else
  err "need either sha256sum or shasum on PATH"
fi

# ---- detect target ---------------------------------------------------------

OS_RAW="$(uname -s)"
ARCH_RAW="$(uname -m)"
case "$OS_RAW" in
  Darwin) OS="darwin" ;;
  Linux)  OS="linux"  ;;
  MINGW*|MSYS*|CYGWIN*) err "Windows detected; please download the .zip from https://github.com/$REPO/releases manually." ;;
  *) err "unsupported OS: $OS_RAW" ;;
esac
case "$ARCH_RAW" in
  arm64|aarch64) ARCH="arm64" ;;
  x86_64|amd64)  ARCH="x64"   ;;
  *) err "unsupported architecture: $ARCH_RAW" ;;
esac
ASSET="contextos-${OS}-${ARCH}.tar.gz"

# ---- resolve release URL ---------------------------------------------------

if [[ "$VERSION" == "latest" ]]; then
  DOWNLOAD_BASE="https://github.com/${REPO}/releases/latest/download"
else
  DOWNLOAD_BASE="https://github.com/${REPO}/releases/download/${VERSION}"
fi
URL="${DOWNLOAD_BASE}/${ASSET}"
SUMS_URL="${DOWNLOAD_BASE}/SHA256SUMS"

# ---- download + verify -----------------------------------------------------

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

info "Downloading $ASSET from $REPO ($VERSION)…"
curl -fsSL "$URL" -o "$TMP/$ASSET" || err "download failed: $URL"

info "Fetching SHA256SUMS for verification…"
if curl -fsSL "$SUMS_URL" -o "$TMP/SHA256SUMS" 2>/dev/null; then
  EXPECTED="$(grep " ${ASSET}\$" "$TMP/SHA256SUMS" | awk '{print $1}')"
  if [[ -n "$EXPECTED" ]]; then
    ACTUAL="$(cd "$TMP" && $SHACMD "$ASSET" | awk '{print $1}')"
    if [[ "$EXPECTED" != "$ACTUAL" ]]; then
      err "checksum mismatch for $ASSET (expected $EXPECTED, got $ACTUAL)"
    fi
    ok "checksum verified"
  else
    info "no entry for $ASSET in SHA256SUMS — skipping checksum (release may predate checksums)"
  fi
else
  info "no SHA256SUMS file in release — skipping checksum"
fi

# ---- install ---------------------------------------------------------------

info "Installing to $INSTALL_DIR/contextos"
mkdir -p "$INSTALL_DIR"
tar -xzf "$TMP/$ASSET" -C "$TMP"
mv "$TMP/contextos" "$INSTALL_DIR/contextos"
chmod +x "$INSTALL_DIR/contextos"

INSTALLED_VERSION="$("$INSTALL_DIR/contextos" version 2>/dev/null || echo unknown)"
ok "Installed contextos $INSTALLED_VERSION"

# ---- PATH advice -----------------------------------------------------------

PATH_NEEDS_UPDATE=0
case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *) PATH_NEEDS_UPDATE=1 ;;
esac

if [[ "$PATH_NEEDS_UPDATE" -eq 1 ]]; then
  cat <<EOF

⚠️  $INSTALL_DIR is not on your PATH.
Add it to your shell config (then restart the shell):

  echo 'export PATH="\$HOME/.local/bin:\$PATH"' >> ~/.zshrc   # zsh (macOS default)
  echo 'export PATH="\$HOME/.local/bin:\$PATH"' >> ~/.bashrc  # bash

EOF
fi

# ---- auto-init the current project (or CONTEXTOS_INIT_DIR) -----------------

# Decide whether and where to auto-init.
INIT_DIR=""
if [[ "${CONTEXTOS_NO_INIT:-0}" == "1" ]]; then
  :
elif [[ -n "${CONTEXTOS_INIT_DIR:-}" ]]; then
  INIT_DIR="$CONTEXTOS_INIT_DIR"
else
  # Treat $PWD as a project if it contains any of the usual project
  # markers. Conservative — random home directory shouldn't trigger init.
  for marker in .git package.json Cargo.toml pyproject.toml go.mod pom.xml composer.json Gemfile build.gradle setup.py requirements.txt; do
    if [[ -e "$PWD/$marker" ]]; then
      INIT_DIR="$PWD"
      break
    fi
  done
fi

if [[ -n "$INIT_DIR" ]]; then
  info "Wiring ContextOS into project at $INIT_DIR"
  INIT_FLAGS=(--root "$INIT_DIR")
  if [[ "${CONTEXTOS_SKIP_BUILD:-0}" == "1" ]]; then
    INIT_FLAGS+=(--skip-build)
  fi
  if "$INSTALL_DIR/contextos" init "${INIT_FLAGS[@]}"; then
    cat <<EOF

✅ Setup complete. Open Claude Code in:
   $INIT_DIR

ContextOS is wired via .mcp.json — every AI request will be token-reduced
automatically. Optional live graph updates: \`contextos watch --root .\`

EOF
  else
    cat <<EOF

⚠️  Binary installed but auto-init failed. You can wire a project manually:
   cd /path/to/your/repo
   contextos init

EOF
  fi
else
  cat <<EOF

Binary installed. ContextOS does not appear to be running from a project
directory, so wiring was skipped. To enable in a project:

   cd /path/to/your/repo
   contextos init

(That's the equivalent of \`contextos build\` + \`contextos install\` in one
command. Open Claude Code afterwards and ContextOS is active.)

EOF
fi
