#!/bin/sh
set -e

REPO="Oeditus/dllb"
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"

detect_os() {
    OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
    case "$OS" in
        linux) OS="unknown-linux-gnu" ;;
        darwin) OS="apple-darwin" ;;
        *) echo "Unsupported OS: $OS"; exit 1 ;;
    esac
}

detect_arch() {
    ARCH="$(uname -m)"
    case "$ARCH" in
        x86_64|amd64) ARCH="x86_64" ;;
        aarch64|arm64) ARCH="aarch64" ;;
        *) echo "Unsupported architecture: $ARCH"; exit 1 ;;
    esac
}

detect_os
detect_arch

if [ "$OS" = "apple-darwin" ] && [ "$ARCH" = "x86_64" ]; then
    echo "Unsupported architecture: x86_64-apple-darwin binaries are not provided in releases."
    exit 1
fi

TARGET="${ARCH}-${OS}"
TAG=$(curl -s "https://api.github.com/repos/${REPO}/releases/latest" \
    | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p')

if [ -z "$TAG" ]; then
    TAG="v1.2.0"
fi

ARCHIVE="dllb-${TAG}-${TARGET}.tar.gz"
URL="https://github.com/${REPO}/releases/download/${TAG}/${ARCHIVE}"
SUMS_URL="https://github.com/${REPO}/releases/download/${TAG}/SHA256SUMS"

TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT

echo "Downloading dllb ${TAG} for ${TARGET}..."
curl -fsSL "$URL" -o "${TMP_DIR}/${ARCHIVE}"

# Verify the artifact against the published checksums before installing.
if curl -fsSL "$SUMS_URL" -o "${TMP_DIR}/SHA256SUMS" 2>/dev/null; then
    expected=$(sed -n "s/^\([0-9a-f]*\)  .*${ARCHIVE}\$/\1/p" "${TMP_DIR}/SHA256SUMS")
    if [ -n "$expected" ]; then
        actual=$(sha256sum "${TMP_DIR}/${ARCHIVE}" 2>/dev/null | cut -d' ' -f1 \
            || shasum -a 256 "${TMP_DIR}/${ARCHIVE}" | cut -d' ' -f1)
        if [ "$expected" != "$actual" ]; then
            echo "Checksum mismatch for ${ARCHIVE}!" >&2
            echo "  expected: $expected" >&2
            echo "  actual:   $actual" >&2
            exit 1
        fi
        echo "Checksum verified."
    fi
else
    echo "Warning: SHA256SUMS unavailable; skipping checksum verification." >&2
fi

echo "Extracting..."
tar -xzf "${TMP_DIR}/${ARCHIVE}" -C "$TMP_DIR"

echo "Installing to ${INSTALL_DIR} (requires write permission)..."
if [ -w "$INSTALL_DIR" ]; then
    cp "${TMP_DIR}/dllb-${TAG}-${TARGET}/dllb-server" "$INSTALL_DIR/"
    cp "${TMP_DIR}/dllb-${TAG}-${TARGET}/dllb-cli" "$INSTALL_DIR/"
else
    sudo cp "${TMP_DIR}/dllb-${TAG}-${TARGET}/dllb-server" "$INSTALL_DIR/"
    sudo cp "${TMP_DIR}/dllb-${TAG}-${TARGET}/dllb-cli" "$INSTALL_DIR/"
fi

echo "Successfully installed dllb-server and dllb-cli to ${INSTALL_DIR}"
