# Voice Server for RK3568

Offline voice server with ASR (speech-to-text) + TTS (text-to-speech) + VAD (voice activity detection) over WebSocket, optimized for Rockchip RK3568 (ARM Cortex-A55).

Powered by [sherpa-onnx](https://github.com/k2-fsa/sherpa-onnx).

---

## Architecture

```
┌─────────────┐     WebSocket      ┌──────────────────────────────┐
│  Client     │ ──────────────────→ │  voice-server (axum)         │
│  (PC/phone) │ ←────────────────── │  ┌─────┐ ┌─────┐ ┌───────┐ │
└─────────────┘     JSON/binary     │  │ ASR │ │ TTS │ │  VAD  │ │
                                    │  │     │ │     │ │(per-  │ │
                                    │  │     │ │     │ │ conn) │ │
                                    │  └─────┘ └─────┘ └───────┘ │
                                    │         sherpa-onnx          │
                                    └──────────────────────────────┘
```

---

## Requirements

- **Hardware**: RK3568 (e.g., FriendlyElec NanoPi R5C/R5S, Orange Pi 5, Rock 3A), or any aarch64 Linux device
- **OS**: Debian/Ubuntu-based Linux (aarch64)
- **Storage**: ~1GB free for models (ASR + TTS + VAD)
- **Runtime**: systemd, Node.js 18+ (for model download script)

---

## Installation

### 1. Install Rust (aarch64)

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
```

### 2. Build

```bash
git clone https://github.com/stevenke1981/voice-server-rk3568.git
cd voice-server-rk3568
cargo build --release
```

### 3. Deploy

```bash
sudo bash deploy/install.sh --all
```

This installs the binary + config + systemd service, and downloads all models.

Or step by step:

```bash
# Install binary + config + service
sudo bash deploy/install.sh

# Download models
sudo MODEL_DIR=/opt/voice-server/models node scripts/download-models.mjs --all

# Start service
sudo systemctl start voice-server
```

### 4. Verify

```bash
# Check service status
sudo systemctl status voice-server

# View logs
sudo journalctl -u voice-server -f

# Expected output:
#   All engines initialized successfully (VAD will be per-connection)
#   Voice server starting on 0.0.0.0:8081
```

---

## Model Options

| Category | Model | Size | Description |
|----------|-------|------|-------------|
| ASR | `zipformer-zh-14m` | ~25MB | Chinese streaming, RK3568 recommended |
| ASR | `zipformer-zh-int8` | ~40MB | Chinese int8, higher accuracy |
| ASR | `sense-voice-int8` | ~228MB | Multilingual (zh/en/ja/ko/yue) |
| ASR | `zipformer-en-20m` | ~40MB | English streaming |
| TTS | `vits-melo-zh-en` | ~163MB | Chinese + English, 1 speaker |
| TTS | `vits-ljs-en` | ~109MB | English (LJSpeech), 1 speaker |
| TTS | `vits-glados-en` | ~61MB | English (GLaDOS), 1 speaker |
| VAD | `silero-vad` | ~208KB | Voice Activity Detection |

```bash
# List available models
node scripts/download-models.mjs --list

# Selective download
sudo MODEL_DIR=/opt/voice-server/models node scripts/download-models.mjs --asr --tts --vad

# Choose specific ASR model
sudo MODEL_DIR=/opt/voice-server/models node scripts/download-models.mjs --asr --asr-model sense-voice-int8
```

---

## WebSocket API

### Endpoint

```
ws://<server-ip>:8081/ws
```

### Message Types

**Client → Server** (JSON):

```json
{ "type": "ping",                "timestamp": 1700000000000 }
{ "type": "asr_start",          "language": null }
{ "type": "asr_audio",          "data": "<base64-pcm>",    "sample_rate": null }
{ "type": "asr_stop" }
{ "type": "tts_request",        "text": "你好世界",       "voice": null }
{ "type": "tts_cancel" }
{ "type": "config",             "key": "vad_threshold",   "value": 0.3 }
```

**Server → Client** (JSON):

```json
{ "type": "pong",                "timestamp": 1700000000000 }
{ "type": "asr_interim",        "text": "你好",         "is_final": false }
{ "type": "asr_final",          "text": "你好世界",     "is_final": true, "confidence": 0.95 }
{ "type": "vad_state",          "state": "speech" }
{ "type": "tts_audio",          "data": "<base64-wav>",  "format": "wav", "sample_rate": 24000 }
{ "type": "tts_end",            "duration_ms": 1500 }
{ "type": "error",              "code": "PARSE_ERROR",   "message": "..." }
```

**Binary frames** (for low-latency ASR):

| Marker | Direction | Content |
|--------|-----------|---------|
| `0x00` | Client → Server | Raw PCM `i16` samples (mono, 16kHz) |
| `0x01` | Server → Client | Raw PCM `i16` samples (mono) |

---

## Client CLI

```bash
# Interactive mode (stdin commands)
cargo run --bin client

# TTS synthesis → save WAV
cargo run --bin client -- --tts "你好世界" -o hello.wav

# ASR from PCM file → save text
cargo run --bin client -- --asr-file speech_16k.pcm -o result.txt

# ASR from WAV file (auto-converted to 16kHz mono)
cargo run --bin client -- --asr-file recording.wav -o result.txt

# ASR streaming mode (chunked every 100ms)
cargo run --bin client -- --asr-file speech.pcm --asr-chunk-ms 100

# Single-shot ASR (no chunking)
cargo run --bin client -- --asr-file speech.pcm --asr-chunk-ms 0

# Custom server URL
cargo run --bin client -- --url ws://192.168.1.100:8081/ws --asr-file speech.pcm
```

### Interactive Commands

```
asr_start [lang]          Start ASR session
asr_stop                  Stop ASR session
asr_audio <base64>        Send base64 PCM audio
asr_audio_file <path>     Send binary PCM file
tts <text>                Synthesize speech
tts_cancel                Cancel TTS
ping                      Heartbeat
config <key> <value>      Update config (e.g., config vad_threshold 0.3)
save <path>               Save last result
help                      Show help
quit                      Exit
```

---

## Configuration

Edit `/opt/voice-server/config.toml`:

```toml
[server]
host = "0.0.0.0"
port = 8081
max_connections = 8

[asr]
model_type = "zipformer"       # zipformer | sense_voice
encoder = "/opt/voice-server/models/asr/encoder.onnx"
decoder = "/opt/voice-server/models/asr/decoder.onnx"
joiner = "/opt/voice-server/models/asr/joiner.onnx"
tokens = "/opt/voice-server/models/asr/tokens.txt"
# For SenseVoice:
# model_type = "sense_voice"
# model = "/opt/voice-server/models/asr/model.int8.onnx"

[tts]
model_type = "vits"
model = "/opt/voice-server/models/tts/model.onnx"
tokens = "/opt/voice-server/models/tts/tokens.txt"

[vad]
model = "/opt/voice-server/models/vad/silero_vad.onnx"
threshold = 0.5
min_speech_duration_ms = 100
min_silence_duration_ms = 500
```

Restart after config change:

```bash
sudo systemctl restart voice-server
```

---

## For Agent / AI (Quick Start)

```bash
# 1. Clone & build
git clone https://github.com/stevenke1981/voice-server-rk3568.git && cd voice-server-rk3568
source "$HOME/.cargo/env" && cargo build --release

# 2. Deploy + download models + start
echo "co1014" | sudo -S bash deploy/install.sh --all

# 3. Wait for model loading (~30s), then test
sleep 35
sudo journalctl -u voice-server --no-pager -n 5

# 4. WebSocket test
sudo apt-get install -y node-ws 2>/dev/null || npm install --no-save ws
node -e "
const {WebSocket} = require('ws');
const ws = new WebSocket('ws://localhost:8081/ws');
ws.on('open', () => ws.send(JSON.stringify({type:'ping',timestamp:Date.now()})));
ws.on('message', d => { console.log(d.toString()); ws.close(); });
ws.on('error', e => console.log(e.message));
"

# 5. ASR test with a 16kHz 16-bit mono PCM file
cargo run --bin client -- --asr-file /path/to/speech_16k.pcm -o result.txt

# 6. TTS test
cargo run --bin client -- --tts "你好世界" -o hello.wav
```

---

## Verification

```bash
# Run all project checks
node scripts/check.mjs
```

---

## 中文說明 (Traditional Chinese)

適用於 RK3568 的離線語音伺服器，支援語音辨識 (ASR)、語音合成 (TTS)、語音活動偵測 (VAD)，透過 WebSocket 提供服務。

### 安裝步驟

```bash
# 1. 下載並編譯
git clone https://github.com/stevenke1981/voice-server-rk3568.git
cd voice-server-rk3568
source "$HOME/.cargo/env"
cargo build --release

# 2. 一鍵部署（安裝 binary + 設定檔 + systemd 服務 + 下載模型）
sudo bash deploy/install.sh --all
```

或逐步安裝：

```bash
# 安裝 binary、設定檔、systemd 服務
sudo bash deploy/install.sh

# 下載模型（中文 ASR + 中英 TTS + VAD）
sudo MODEL_DIR=/opt/voice-server/models node scripts/download-models.mjs --all

# 啟動服務
sudo systemctl start voice-server

# 查看狀態
sudo journalctl -u voice-server -f
```

### 模型選擇

```bash
# 列出可用模型
node scripts/download-models.mjs --list

# 下載指定 ASR 模型（中文，RK3568 推薦）
sudo MODEL_DIR=/opt/voice-server/models node scripts/download-models.mjs --asr

# 下載更高精度的中文模型
sudo MODEL_DIR=/opt/voice-server/models node scripts/download-models.mjs --asr --asr-model zipformer-zh-int8

# 下載多語言模型（中/英/日/韓/粵）
sudo MODEL_DIR=/opt/voice-server/models node scripts/download-models.mjs --asr --asr-model sense-voice-int8
```

### WebSocket 端點

```
ws://<RK3568-IP>:8081/ws
```

### 客戶端使用

```bash
# 語音合成（TTS）：將文字轉為語音 WAV 檔
cargo run --bin client -- --tts "你好世界" -o hello.wav

# 語音辨識（ASR）：將 16kHz 16-bit PCM 檔轉為文字
cargo run --bin client -- --asr-file speech_16k.pcm -o result.txt

# ASR：直接餵 WAV 檔（自動轉換為 16kHz）
cargo run --bin client -- --asr-file recording.wav -o result.txt

# 指定伺服器位置
cargo run --bin client -- --url ws://192.168.80.213:8081/ws --asr-file speech.pcm -o result.txt
```

### 互動模式

直接執行 `cargo run --bin client` 進入互動模式，支援以下指令：

| 指令 | 說明 |
|------|------|
| `asr_start` | 開始語音辨識 |
| `asr_stop` | 停止語音辨識 |
| `asr_audio <base64>` | 發送 base64 PCM 音訊 |
| `asr_audio_file <path>` | 發送 PCM 音訊檔 |
| `tts <text>` | 語音合成 |
| `ping` | 心跳測試 |
| `config <key> <value>` | 更新設定（例：`config vad_threshold 0.3`） |
| `save <path>` | 儲存最後結果 |
| `help` | 顯示說明 |
| `quit` | 離開 |

### 設定檔

路徑：`/opt/voice-server/config.toml`

```toml
[server]
host = "0.0.0.0"
port = 8081

[asr]
model_type = "zipformer"
encoder = "/opt/voice-server/models/asr/encoder.onnx"
decoder = "/opt/voice-server/models/asr/decoder.onnx"
joiner = "/opt/voice-server/models/asr/joiner.onnx"
tokens = "/opt/voice-server/models/asr/tokens.txt"

[tts]
model_type = "vits"
model = "/opt/voice-server/models/tts/model.onnx"
tokens = "/opt/voice-server/models/tts/tokens.txt"

[vad]
model = "/opt/voice-server/models/vad/silero_vad.onnx"
threshold = 0.5
```

修改後重啟：`sudo systemctl restart voice-server`

### 給 AI Agent 的快速部署指令

```bash
git clone https://github.com/stevenke1981/voice-server-rk3568.git && cd voice-server-rk3568
source "$HOME/.cargo/env" && cargo build --release
echo "co1014" | sudo -S bash deploy/install.sh --all
sleep 35 && sudo journalctl -u voice-server --no-pager -n 5
# 測試 ping/pong
node -e "const {WebSocket}=require('ws'); const ws=new WebSocket('ws://localhost:8081/ws'); ws.on('open',()=>ws.send(JSON.stringify({type:'ping',timestamp:Date.now()}))); ws.on('message',d=>{console.log(d.toString());ws.close();});"
# 測試 TTS
cargo run --bin client -- --tts "你好世界" -o hello.wav
```

---

## Files Reference

| File | Purpose |
|------|---------|
| `config.toml` | Server configuration |
| `deploy/install.sh` | One-click deployment script |
| `deploy/voice-server.service` | systemd unit file |
| `scripts/download-models.mjs` | Model download helper |
| `scripts/check.mjs` | Project verification (62 checks) |
| `src/main.rs` | Server entry point |
| `src/ws/handler.rs` | WebSocket connection handler |
| `src/ws/protocol.rs` | WebSocket message types |
| `src/bin/client.rs` | CLI test client |

---

