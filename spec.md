# Spec — RK3568 Rust 語音服務器 (ASR + TTS)

## 1. 產品概述

一個在 RK3568 Arm board（Armbian Linux）上運行的離線語音服務器。
支援 WebSocket 雙向通訊，提供串流語音辨識（ASR）與語音合成（TTS），
支援多路客戶端同時連線，全部推理在本地端完成，不需網際網路。

## 2. 硬體需求

| 項目 | 最低要求 | 建議 |
|---|---|---|
| SoC | RK3568 | RK3568 |
| CPU | 4× Cortex-A55 @ 1.8 GHz | 4× Cortex-A55 @ 2.0 GHz |
| RAM | 2 GB | **4 GB+** |
| 儲存 | 8 GB eMMC (可用空間 > 1 GB) | 32 GB+ eMMC / SD |
| 網路 | 100 Mbps Ethernet | Gigabit Ethernet |
| OS | Armbian Linux (kernel 6.x+) | Armbian Linux (kernel 6.x+) |
| 執行環境 | Rust runtime (musl/glibc) | 同左 |

## 3. 功能需求

### F1：串流語音辨識 (ASR)

| ID | 需求 | 優先級 |
|---|---|---|
| F1.1 | 用戶端通過 WebSocket 傳送 PCM 音訊 chunk → 服務器即時回傳辨識文字 | P0 |
| F1.2 | 支援取樣率 16 kHz、16-bit、單聲道 PCM | P0 |
| F1.3 | 支援**中間結果**（interim）隨音訊流入逐步更新 | P0 |
| F1.4 | 語音結束後回傳**最終結果**（final）含信心度 | P0 |
| F1.5 | 支援 SenseVoice / Zipformer / Paraformer 等 sherpa-onnx 支援的模型 | P1 |
| F1.6 | 支援中英文（依模型而定） | P0 |
| F1.7 | 可配置 ASR 模型切換（runtime 重載） | P2 |

### F2：語音活動偵測 (VAD)

| ID | 需求 | 優先級 |
|---|---|---|
| F2.1 | 使用 Silero VAD 偵測語音起止 | P0 |
| F2.2 | 回傳 `speech` / `silence` 狀態給客戶端 | P1 |
| F2.3 | 可配置 VAD 敏感度（threshold） | P2 |
| F2.4 | 自動偵測語音結束後觸發 ASR 最終結果 | P1 |

### F3：語音合成 (TTS)

| ID | 需求 | 優先級 |
|---|---|---|
| F3.1 | 客戶端傳送文字 → 服務器回傳合成的 PCM 音訊 | P0 |
| F3.2 | 支援多種語音（多模型切換） | P1 |
| F3.3 | 支援 chunked streaming 回傳（邊合成邊送） | P0 |
| F3.4 | 支援 Piper / Kokoro / Matcha-TTS / VITS 模型 | P1 |
| F3.5 | TTS 音訊預設 24 kHz、16-bit、單聲道 PCM | P0 |

### F4：服務管理

| ID | 需求 | 優先級 |
|---|---|---|
| F4.1 | 支援多客戶端同時連線（至少 4 路） | P0 |
| F4.2 | 每路連線獨立 ASR session / TTS session | P0 |
| F4.3 | 支援 systemd service 管理 | P1 |
| F4.4 | 設定檔（TOML）指定模型路徑、網路埠等 | P0 |
| F4.5 | 記錄 basic 存取日誌（連線/斷線/錯誤） | P2 |

## 4. 非功能需求

| ID | 需求 | 目標值 |
|---|---|---|
| NFR1 | ASR 延遲（Real-Time Factor） | RTF < 0.5（單路） |
| NFR2 | TTS 延遲（首次 chunk） | < 500 ms |
| NFR3 | 最大同時連線數 | ≥ 4 路 |
| NFR4 | 服務器啟動時間 | < 5 秒 |
| NFR5 | 二進制檔大小 | < 50 MB（不含模型） |
| NFR6 | 模型儲存空間 | < 500 MB |
| NFR7 | CPU 使用率（閒置） | < 5% |
| NFR8 | 記憶體使用率（閒置/滿載） | < 200 MB / < 1 GB |
| NFR9 | WebSocket 訊息格式 | JSON + 二進制 frame |
| NFR10 | 離線運行 | 完全不需要網際網路 |

## 5. WebSocket API 規範

### 5.1 連線
```
ws://<rk3568-ip>:8080/ws
```

### 5.2 服務器 → 客戶端訊息

| type | 說明 | payload 欄位 |
|---|---|---|
| `asr_interim` | ASR 中間結果 | `text: string`, `is_final: false` |
| `asr_final` | ASR 最終結果 | `text: string`, `is_final: true`, `confidence: f32` |
| `asr_error` | ASR 錯誤 | `code: string`, `message: string` |
| `tts_audio` | TTS 音訊 chunk | `data: bytes`(binary frame) 或 `data: string`(base64), `format: string`, `sample_rate: u32` |
| `tts_end` | TTS 合成結束 | `duration_ms: u32` |
| `tts_error` | TTS 錯誤 | `code: string`, `message: string` |
| `vad_state` | VAD 狀態變化 | `state: "speech" \| "silence"` |
| `error` | 一般錯誤 | `code: string`, `message: string` |
| `pong` | 心跳回應 | `timestamp: u64` |

### 5.3 客戶端 → 服務器訊息

| type | 說明 | payload 欄位 |
|---|---|---|
| `asr_audio` | ASR 音訊資料 | `data: bytes`(binary binary frame) 或 `data: string`(base64), `sample_rate: u32` (可選，預設 16000) |
| `asr_start` | 開始 ASR session | `language: string` (可選) |
| `asr_stop` | 結束 ASR session | 無 |
| `tts_request` | 請求 TTS | `text: string`, `voice: string` (可選) |
| `tts_cancel` | 取消目前 TTS | 無 |
| `config` | 更新設定 | `key: string`, `value: any` |
| `ping` | 心跳 | `timestamp: u64` |

### 5.4 二進制 Frame 規則

ASR 音訊使用 **binary WebSocket frames** 傳送（非 base64 JSON）以減少開銷：

```
Binary Frame (音訊):
  [0x00]  ← ASR audio marker byte
  [PCM data...]

Binary Frame (TTS 音訊回傳):
  [0x01]  ← TTS audio marker byte
  [PCM data...]
```

文字訊息使用 **Text WebSocket frames**（JSON）。

## 6. 模型規格

### 6.1 ASR 模型（至少擇一）

| 模型 | 語言 | 大小 (MB) | INT8 | 串流 |
|---|---|---|---|---|
| Zipformer-EN-20M | 英文 | ~20 | ✅ | ✅ |
| Zipformer-ZH-20M | 中文 | ~20 | ✅ | ✅ |
| SenseVoice Small | 中/英/日/韓/粵 | ~240 | ❌ | ✅ |
| Paraformer-small | 中文 | ~50 | ✅ | ❌ (offline) |

### 6.2 TTS 模型（至少擇一）

| 模型 | 語言 | 大小 (MB) | 品質 |
|---|---|---|---|
| Piper-EN | 英文 | ~10-50 | 中高 |
| Piper-ZH | 中文 | ~20-50 | 中高 |
| Kokoro-82M | 多語 | ~80 | 高 |
| Matcha-TTS | 多語 | ~50-100 | 高 |

### 6.3 VAD 模型

| 模型 | 大小 |
|---|---|
| Silero VAD v5 INT8 | ~5 MB |

## 7. 部署架構

```
RK3568 Armbian Linux
├── /opt/voice-server/
│   ├── voice-server          # 靜態連結二進制檔 (~30MB)
│   ├── config.toml           # 設定檔
│   └── models/
│       ├── asr/              # ASR 模型檔
│       ├── tts/              # TTS 模型檔
│       └── vad/              # VAD 模型檔
│
├── /etc/systemd/system/
│   └── voice-server.service  # systemd service
│
└── /var/log/voice-server/    # 日誌
```

## 8. 設定檔格式 (config.toml)

```toml
[server]
host = "0.0.0.0"
port = 8080
max_connections = 8

[asr]
model_type = "zipformer"
encoder = "/opt/voice-server/models/asr/encoder.onnx"
decoder = "/opt/voice-server/models/asr/decoder.onnx"
joiner = "/opt/voice-server/models/asr/joiner.onnx"
tokens = "/opt/voice-server/models/asr/tokens.txt"
num_threads = 2
provider = "cpu"

[tts]
model_type = "piper"
model = "/opt/voice-server/models/tts/model.onnx"
tokens = "/opt/voice-server/models/tts/tokens.txt"
num_threads = 1
provider = "cpu"

[vad]
model = "/opt/voice-server/models/vad/silero_vad.onnx"
threshold = 0.5
min_speech_duration_ms = 100
min_silence_duration_ms = 500
window_size = 512  # samples
```

## 9. 約束與限制

1. 每路 WebSocket 連線使用獨立的 `OnlineRecognizer` stream（非 thread-safe，但可 clone config）
2. sherpa-onnx 的 OnlineRecognizer 不是 `Send + Sync`，需要用 `tokio::sync::Mutex` 保護或 per-connection instance
3. 模型檔案在初始化時載入，如要熱切換需要重新建立 recognizer instance
4. 所有客戶端共享同一組模型檔案（memory-mapped 可節省 RAM）
