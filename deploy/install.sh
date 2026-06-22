#!/usr/bin/env bash
#
# install.sh — One-click deployment script for voice-server on RK3568
#
# Usage:
#   sudo bash install.sh [--model-dir /path/to/models]
#
set -euo pipefail

BIN_NAME="voice-server"
CONFIG_NAME="config.toml"
INSTALL_DIR="/opt/voice-server"
MODEL_DIR="${INSTALL_DIR}/models"
SYSTEMD_DIR="/etc/systemd/system"
SERVICE_NAME="voice-server.service"

echo "=== Voice Server Installer for RK3568 ==="

# Ensure running as root
if [ "$(id -u)" -ne 0 ]; then
    echo "Please run with sudo or as root."
    exit 1
fi

# Create directory structure
mkdir -p "${INSTALL_DIR}"
mkdir -p "${MODEL_DIR}/asr"
mkdir -p "${MODEL_DIR}/tts"
mkdir -p "${MODEL_DIR}/vad"
mkdir -p "/var/log/voice-server"

# Copy binary
if [ -f "./${BIN_NAME}" ]; then
    cp "./${BIN_NAME}" "${INSTALL_DIR}/${BIN_NAME}"
    chmod +x "${INSTALL_DIR}/${BIN_NAME}"
    echo "✓ Binary installed: ${INSTALL_DIR}/${BIN_NAME}"
else
    echo "⚠ Binary '${BIN_NAME}' not found in current directory."
    echo "  Please build it first and place it here."
fi

# Copy config
if [ -f "./${CONFIG_NAME}" ]; then
    cp "./${CONFIG_NAME}" "${INSTALL_DIR}/${CONFIG_NAME}"
    echo "✓ Config installed: ${INSTALL_DIR}/${CONFIG_NAME}"
else
    echo "⚠ Config file '${CONFIG_NAME}' not found."
    echo "  A default will be created at runtime."
fi

# Install systemd service
if [ -f "./${SERVICE_NAME}" ]; then
    cp "./${SERVICE_NAME}" "${SYSTEMD_DIR}/${SERVICE_NAME}"
    chmod 644 "${SYSTEMD_DIR}/${SERVICE_NAME}"
    systemctl daemon-reload
    echo "✓ Systemd service installed: ${SERVICE_NAME}"
else
    echo "⚠ Service file '${SERVICE_NAME}' not found."
    echo "  Create it manually or skip systemd integration."
fi

# Model download helper
echo ""
echo "=== Model Setup ==="
echo "Place your model files in:"
echo "  ASR: ${MODEL_DIR}/asr/"
echo "  TTS: ${MODEL_DIR}/tts/"
echo "  VAD: ${MODEL_DIR}/vad/"
echo ""
echo "Then update ${INSTALL_DIR}/${CONFIG_NAME} with correct paths."
echo ""
echo "=== Ready ==="
echo "Start the service with:"
echo "  sudo systemctl start voice-server"
echo ""
echo "Check status with:"
echo "  sudo systemctl status voice-server"
