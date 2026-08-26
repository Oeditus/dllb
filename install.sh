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
TAG=$(curl -s "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')

if [ -z "$TAG" ]; then
    TAG="v1.0.0"
fi

ARCHIVE="dllb-${TAG}-${TARGET}.tar.gz"
URL="https://github://${REPO}/releases/download/${TAG}/${ARCHIVE}"

TMP_DIR=$(mktemp -d)
trap 'rm -rf "$TMP_DIR"' EXIT

echo "Downloading dllb ${TAG} for ${TARGET}..."
curl -fsSL "$URL" -o "${TMP_DIR}/${ARCHIVE}"

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
