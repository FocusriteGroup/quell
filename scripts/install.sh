#!/bin/sh
# install.sh - Install quell from GitHub Releases
# Usage: curl -fsSL https://raw.githubusercontent.com/FurbySoup/quell/main/scripts/install.sh | sh
set -eu

REPO="FurbySoup/quell"
INSTALL_DIR="$HOME/.local/bin"
BINARY_NAME="quell"

log()  { printf '%s\n' "$1"; }
err()  { printf 'error: %s\n' "$1" >&2; exit 1; }

# --- Detect OS ---
OS="$(uname -s)"
case "$OS" in
    Darwin)  OS_LABEL="macos"  ;;
    Linux)   OS_LABEL="linux"  ;;
    *)       err "Unsupported operating system: $OS (only macOS and Linux are supported)" ;;
esac

# --- Detect architecture ---
ARCH="$(uname -m)"
case "$ARCH" in
    aarch64|arm64) ARCH_LABEL="aarch64" ;;
    x86_64|amd64)  ARCH_LABEL="x86_64"  ;;
    *)             err "Unsupported architecture: $ARCH" ;;
esac

ASSET_NAME="${BINARY_NAME}-${OS_LABEL}-${ARCH_LABEL}"
log "Detected platform: ${OS_LABEL} ${ARCH_LABEL}"

# --- Resolve latest release tag ---
log "Fetching latest release..."
RELEASE_URL="https://api.github.com/repos/${REPO}/releases/latest"

if command -v curl >/dev/null 2>&1; then
    RELEASE_JSON="$(curl -fsSL "$RELEASE_URL")" || err "Failed to fetch release info. Check your internet connection."
elif command -v wget >/dev/null 2>&1; then
    RELEASE_JSON="$(wget -qO- "$RELEASE_URL")" || err "Failed to fetch release info. Check your internet connection."
else
    err "Neither curl nor wget found. Please install one and try again."
fi

# Extract tag name (works without jq)
TAG="$(printf '%s' "$RELEASE_JSON" | grep '"tag_name"' | head -1 | sed 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/')"
[ -z "$TAG" ] && err "Could not determine latest release tag."
log "Latest release: ${TAG}"

# --- Build download URLs ---
BASE_DOWNLOAD="https://github.com/${REPO}/releases/download/${TAG}"
BINARY_URL="${BASE_DOWNLOAD}/${ASSET_NAME}"
CHECKSUM_URL="${BASE_DOWNLOAD}/${ASSET_NAME}.sha256"

# --- Download binary and checksum ---
TMPDIR="$(mktemp -d)"
trap 'rm -rf "$TMPDIR"' EXIT

log "Downloading ${ASSET_NAME}..."
if command -v curl >/dev/null 2>&1; then
    curl -fSL --progress-bar -o "${TMPDIR}/${ASSET_NAME}" "$BINARY_URL" || err "Failed to download binary. The asset '${ASSET_NAME}' may not exist for this release."
    curl -fsSL -o "${TMPDIR}/${ASSET_NAME}.sha256" "$CHECKSUM_URL" 2>/dev/null || CHECKSUM_AVAILABLE=false
else
    wget -q --show-progress -O "${TMPDIR}/${ASSET_NAME}" "$BINARY_URL" || err "Failed to download binary. The asset '${ASSET_NAME}' may not exist for this release."
    wget -q -O "${TMPDIR}/${ASSET_NAME}.sha256" "$CHECKSUM_URL" 2>/dev/null || CHECKSUM_AVAILABLE=false
fi

# --- Verify checksum ---
CHECKSUM_AVAILABLE="${CHECKSUM_AVAILABLE:-true}"
if [ "$CHECKSUM_AVAILABLE" = true ]; then
    log "Verifying SHA256 checksum..."
    EXPECTED="$(awk '{print $1}' "${TMPDIR}/${ASSET_NAME}.sha256")"

    if command -v sha256sum >/dev/null 2>&1; then
        ACTUAL="$(sha256sum "${TMPDIR}/${ASSET_NAME}" | awk '{print $1}')"
    elif command -v shasum >/dev/null 2>&1; then
        ACTUAL="$(shasum -a 256 "${TMPDIR}/${ASSET_NAME}" | awk '{print $1}')"
    else
        log "Warning: no sha256sum or shasum found, skipping checksum verification."
        ACTUAL="$EXPECTED"
    fi

    if [ "$ACTUAL" != "$EXPECTED" ]; then
        err "Checksum mismatch!\n  Expected: ${EXPECTED}\n  Actual:   ${ACTUAL}\nThe download may be corrupted. Please try again."
    fi
    log "Checksum verified."
else
    log "Warning: no checksum file found for this release, skipping verification."
fi

# --- Install ---
mkdir -p "$INSTALL_DIR"
mv "${TMPDIR}/${ASSET_NAME}" "${INSTALL_DIR}/${BINARY_NAME}"
chmod +x "${INSTALL_DIR}/${BINARY_NAME}"
log "Installed quell to ${INSTALL_DIR}/${BINARY_NAME}"

# --- PATH check ---
case ":${PATH}:" in
    *":${INSTALL_DIR}:"*) ;;
    *)
        log ""
        log "NOTE: ${INSTALL_DIR} is not in your PATH."
        log "Add it by appending one of these to your shell profile:"
        log ""
        log "  # bash (~/.bashrc or ~/.bash_profile)"
        log "  export PATH=\"\$HOME/.local/bin:\$PATH\""
        log ""
        log "  # zsh (~/.zshrc)"
        log "  export PATH=\"\$HOME/.local/bin:\$PATH\""
        log ""
        log "  # fish (~/.config/fish/config.fish)"
        log "  fish_add_path \$HOME/.local/bin"
        log ""
        log "Then restart your shell or run: source <profile>"
        ;;
esac

# --- Done ---
log ""
log "Installation complete! Run 'quell --help' to get started."
log ""
log "Usage:"
log "  quell -- claude        Run Claude Code with scroll-fix"
log "  quell --help           Show all options"
