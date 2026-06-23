#!/usr/bin/env bash
#
# install.sh — One-click deployment script for voice-server on RK3568
#
# Usage: Run from the project root:
#   sudo bash deploy/install.sh [--start] [--download-models]
#
# Options:
#   --start             Also start the service after installation
#   --download-models   Auto-download recommended models to /opt/voice-server/models/
#   --all               Do everything (install + download models + start)
#
# Searches for the binary in this order:
#   1. ./target/release/voice-server  (cargo build --release output)
#   2. ./voice-server                 (copied to project root)
#
set -euo pipefail

# ── Parse flags ───────────────────────────────────────────
FLAG_START=false
FLAG_DOWNLOAD=false
for arg in "$@"; do
    case "${arg}" in
        --start) FLAG_START=true ;;
        --download-models) FLAG_DOWNLOAD=true ;;
        --all) FLAG_START=true; FLAG_DOWNLOAD=true ;;
    esac
done

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

# ── Download models ───────────────────────────────────────
if [ "${FLAG_DOWNLOAD}" = true ]; then
    DOWNLOAD_SCRIPT="${PROJECT_ROOT}/scripts/download-models.mjs"
    if [ -f "${DOWNLOAD_SCRIPT}" ]; then
        echo ""
        echo "=== Downloading models ==="
        MODEL_DIR="${MODEL_DIR}" node "${DOWNLOAD_SCRIPT}" --all
        echo ""

        # Update config.toml paths to match downloaded models
        echo "=== Updating config paths ==="
        if ls "${MODEL_DIR}/asr/" 2>/dev/null | grep -qE 'encoder\.onnx|model\.int8\.onnx'; then
            # Zipformer transducer model (encoder/decoder/joiner)
            if [ -f "${MODEL_DIR}/asr/encoder.onnx" ]; then
                sed -i 's|encoder = ".*"|encoder = "/opt/voice-server/models/asr/encoder.onnx"|' "${INSTALL_DIR}/${CONFIG_NAME}"
                sed -i 's|decoder = ".*"|decoder = "/opt/voice-server/models/asr/decoder.onnx"|' "${INSTALL_DIR}/${CONFIG_NAME}"
                sed -i 's|joiner = ".*"|joiner = "/opt/voice-server/models/asr/joiner.onnx"|' "${INSTALL_DIR}/${CONFIG_NAME}"
                sed -i 's|tokens = ".*"|tokens = "/opt/voice-server/models/asr/tokens.txt"|' "${INSTALL_DIR}/${CONFIG_NAME}"
                sed -i 's|model_type = ".*"|model_type = "zipformer"|' "${INSTALL_DIR}/${CONFIG_NAME}"
                echo "✓ ASR config updated for zipformer transducer model"
            # SenseVoice model (single model file)
            elif [ -f "${MODEL_DIR}/asr/model.int8.onnx" ]; then
                sed -i 's|encoder = ".*"|#encoder = "/opt/voice-server/models/asr/encoder.onnx"|' "${INSTALL_DIR}/${CONFIG_NAME}"
                sed -i 's|decoder = ".*"|#decoder = "/opt/voice-server/models/asr/decoder.onnx"|' "${INSTALL_DIR}/${CONFIG_NAME}"
                sed -i 's|joiner = ".*"|#joiner = "/opt/voice-server/models/asr/joiner.onnx"|' "${INSTALL_DIR}/${CONFIG_NAME}"
                sed -i 's|model_type = ".*"|model_type = "sense_voice"|' "${INSTALL_DIR}/${CONFIG_NAME}"
                if ! grep -q '^model = ' "${INSTALL_DIR}/${CONFIG_NAME}"; then
                    sed -i '/^\[asr\]/a model = "/opt/voice-server/models/asr/model.int8.onnx"' "${INSTALL_DIR}/${CONFIG_NAME}"
                else
                    sed -i 's|model = ".*"|model = "/opt/voice-server/models/asr/model.int8.onnx"|' "${INSTALL_DIR}/${CONFIG_NAME}"
                fi
                echo "✓ ASR config updated for SenseVoice model"
            fi
        fi
        if [ -f "${MODEL_DIR}/vad/silero_vad.onnx" ]; then
            sed -i 's|model = ".*"|model = "/opt/voice-server/models/vad/silero_vad.onnx"|' "${INSTALL_DIR}/${CONFIG_NAME}"
            echo "✓ VAD config path updated"
        fi
        if [ -f "${MODEL_DIR}/tts/model.onnx" ]; then
            sed -i 's|model = ".*"|model = "/opt/voice-server/models/tts/model.onnx"|' "${INSTALL_DIR}/${CONFIG_NAME}"
            echo "✓ TTS config path updated"
        fi
    else
        echo "⚠ Download script not found: ${DOWNLOAD_SCRIPT}"
        echo "  Models must be downloaded manually."
    fi
fi

# ── Ready ─────────────────────────────────────────────────
echo ""
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
if [ "${FLAG_START}" = true ]; then
    echo "Starting voice-server service..."
    if systemctl start voice-server 2>/dev/null; then
        echo "✓ Service started successfully."
        systemctl status voice-server --no-pager | head -10
    else
        echo "⚠ Service failed to start (likely missing model files)."
        echo "  Check logs: sudo journalctl -u voice-server -n 20"
    fi
fi
