#!/usr/bin/env bash
#
# install.sh — One-click deployment script for voice-server on RK3568
#
# Usage: Run from the project root:
#   sudo bash deploy/install.sh [--start]
#
# Options:
#   --start    Also start the service after installation
#
# Searches for the binary in this order:
#   1. ./target/release/voice-server  (cargo build --release output)
#   2. ./voice-server                 (copied to project root)
#
set -euo pipefail

# ── Resolve paths ─────────────────────────────────────────
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

BIN_NAME="voice-server"
CONFIG_NAME="config.toml"
SERVICE_NAME="voice-server.service"

INSTALL_DIR="/opt/voice-server"
MODEL_DIR="${INSTALL_DIR}/models"
SYSTEMD_DIR="/etc/systemd/system"

echo "=== Voice Server Installer for RK3568 ==="
echo "Script dir: ${SCRIPT_DIR}"
echo "Project root: ${PROJECT_ROOT}"
echo "Install dir: ${INSTALL_DIR}"
echo ""

# Ensure running as root
if [ "$(id -u)" -ne 0 ]; then
    echo "Please run with sudo or as root."
    exit 1
fi

# ── Create directory structure ────────────────────────────
mkdir -p "${INSTALL_DIR}"
mkdir -p "${MODEL_DIR}/asr"
mkdir -p "${MODEL_DIR}/tts"
mkdir -p "${MODEL_DIR}/vad"
mkdir -p "/var/log/voice-server"

# ── Locate and install binary ─────────────────────────────
BINARY_SOURCE=""
if [ -f "${PROJECT_ROOT}/target/release/${BIN_NAME}" ]; then
    BINARY_SOURCE="${PROJECT_ROOT}/target/release/${BIN_NAME}"
elif [ -f "${PROJECT_ROOT}/${BIN_NAME}" ]; then
    BINARY_SOURCE="${PROJECT_ROOT}/${BIN_NAME}"
fi

if [ -n "${BINARY_SOURCE}" ]; then
    cp "${BINARY_SOURCE}" "${INSTALL_DIR}/${BIN_NAME}"
    chmod +x "${INSTALL_DIR}/${BIN_NAME}"
    echo "✓ Binary installed: ${INSTALL_DIR}/${BIN_NAME} ($(du -h "${BINARY_SOURCE}" | cut -f1))"
else
    echo "⚠ Binary not found. Build it first:"
    echo "     cargo build --release"
    echo "   Then re-run this script."
fi

# ── Install config ────────────────────────────────────────
if [ -f "${PROJECT_ROOT}/${CONFIG_NAME}" ]; then
    cp "${PROJECT_ROOT}/${CONFIG_NAME}" "${INSTALL_DIR}/${CONFIG_NAME}"
    echo "✓ Config installed: ${INSTALL_DIR}/${CONFIG_NAME}"
else
    echo "⚠ Config file '${CONFIG_NAME}' not found in project root."
fi

# ── Install systemd service ───────────────────────────────
if [ -f "${SCRIPT_DIR}/${SERVICE_NAME}" ]; then
    cp "${SCRIPT_DIR}/${SERVICE_NAME}" "${SYSTEMD_DIR}/${SERVICE_NAME}"
    chmod 644 "${SYSTEMD_DIR}/${SERVICE_NAME}"
    systemctl daemon-reload
    systemctl enable "${SERVICE_NAME}"
    echo "✓ Systemd service installed + enabled: ${SERVICE_NAME}"
else
    echo "⚠ Service file '${SERVICE_NAME}' not found in ${SCRIPT_DIR}/"
fi

# ── Model setup reminder ──────────────────────────────────
echo ""
echo "=== Model Setup ==="
echo "Place your model files in:"
echo "  ASR: ${MODEL_DIR}/asr/"
echo "  TTS: ${MODEL_DIR}/tts/"
echo "  VAD: ${MODEL_DIR}/vad/"
echo ""
echo "Then update ${INSTALL_DIR}/${CONFIG_NAME} with correct paths."
echo "Or use the download helper:"
echo "  node scripts/download-models.mjs --all"
echo ""

# ── Ready ─────────────────────────────────────────────────
echo "=== Ready ==="
echo "Start the service with:"
echo "  sudo systemctl start voice-server"
echo ""
echo "Check status with:"
echo "  sudo systemctl status voice-server"
echo ""
echo "View logs with:"
echo "  sudo journalctl -u voice-server -f"
echo ""

# Optionally start
if [[ "${*}" == *"--start"* ]]; then
    echo "Starting voice-server service..."
    systemctl start voice-server || echo "⚠ Service may not start if model files are missing."
    systemctl status voice-server --no-pager | head -10
fi
