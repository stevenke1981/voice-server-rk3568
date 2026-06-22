# Plan — RK3568 Rust 語音服務器 (ASR + TTS)

## 1. 願景

在 RK3568（Armbian Linux, ARM64）上建立一個純 Rust 的離線語音服務器，提供 WebSocket 介面的 ASR（語音辨識）+ TTS（語音合成）+ VAD（語音活動偵測），全部在本地端運行，不需連接網路。

## 2. 核心策略

### 2.1 不要重造輪子
sherpa-onnx 已提供**官方 Rust crate**（`sherpa-onnx` v1.13.3），包裝了完整的 C API。
我們用 Rust 寫**應用層服務器**，透過 `sherpa-onnx` crate 直接使用其 ASR/TTS/VAD 功能。

### 2.2 Rust 作為膠水層
```
┌─────────────────────────────────────┐
│   rust-voice-server (本專案)         │
│  ┌───────────────────────────────┐  │
│  │ axum HTTP/WS 服務             │  │
│  │ tokio 非同步運行時             │  │
│  ├───────────────────────────────┤  │
│  │ sherpa-onnx Rust crate        │  │  ← crates.io 官方套件
│  ├───────────────────────────────┤  │
│  │ sherpa-onnx C library (static)│  │  ← build script 自動下載
│  └───────────────────────────────┘  │
└─────────────────────────────────────┘
```

### 2.3 Cross-compilation 策略
- 開發在 x86_64 Linux/macOS 上進行
- 使用 `rustup target add aarch64-unknown-linux-gnu`
- 使用 Linaro GCC toolchain 作為 C linker
- `cargo build --target aarch64-unknown-linux-gnu --release`
- sherpa-onnx build script 自動下載 aarch64 的 prebuilt static lib
- 產出單一靜態連結二進制檔，scp 到 RK3568 執行

### 2.4 模型策略
| 功能 | 推薦模型 | 大小 | RTF 推估 | 原因 |
|---|---|---|---|---|
| ASR | Zipformer-EN/ZH streaming | ~20-40MB | < 0.2 | 專為串流設計，低延遲 |
| TTS | Piper-EN/ZH | ~10-50MB | < 0.1 | 極輕量，CPU 即時合成 |
| TTS 進階 | Kokoro-82M | ~80MB | ~0.3 | 更高音質，可選 |
| VAD | Silero VAD | ~5MB | < 0.01 | 標準選擇 |

## 3. 里程碑

### M1：專案 scaffold + 交叉編譯環境（Day 1）
- [ ] 建立 Rust 專案，引入 `sherpa-onnx` crate
- [ ] 確認 aarch64 cross-compilation 路徑
- [ ] 撰寫最小範例（hello ASR），在 RK3568 上實測

### M2：ASR 模組（Day 2-3）
- [ ] OnlineRecognizer 串流辨識
- [ ] 音訊 chunk 餵入 + 即時結果回吐
- [ ] Silero VAD 整合（偵測語音起止）

### M3：TTS 模組（Day 4）
- [ ] OfflineTts 文字轉語音
- [ ] 音訊 chunked 回傳（streaming playback ready）

### M4：WebSocket 服務層（Day 5-6）
- [ ] axum WebSocket endpoint
- [ ] 雙向協定設計（用戶端送音訊→ASR，送文字→TTS）
- [ ] 支援多路連線（tokio per-connection task）

### M5：整合測試 + 部署（Day 7）
- [ ] 全流程 E2E 測試
- [ ] 壓力測試（多路併發）
- [ ] 部署腳本（systemd service）

## 4. 架構設計

```
rust-voice-server/
├── src/
│   ├── main.rs              # entry point, axum server
│   ├── ws/
│   │   ├── handler.rs       # WebSocket connection handler
│   │   └── protocol.rs      # 訊息協定定義 (serde)
│   ├── asr/
│   │   ├── engine.rs        # OnlineRecognizer 封裝
│   │   └── vad.rs           # Silero VAD 封裝
│   ├── tts/
│   │   └── engine.rs        # OfflineTts 封裝
│   └── config.rs            # 設定檔載入
├── models/                   # 模型檔案（在 RK3568 上）
│   ├── asr/
│   ├── tts/
│   └── vad/
├── Cargo.toml
└── deploy/
    ├── install.sh            # 一鍵部署腳本
    └── voice-server.service  # systemd service
```

## 5. WebSocket 協定（草案）

### 用戶端 → 服務器

```json
// ASR 音訊流
{"type": "asr_audio", "data": "<base64 pcm_chunk>", "sample_rate": 16000}

// ASR 停止
{"type": "asr_stop"}

// TTS 請求
{"type": "tts_request", "text": "你好，有什麼可以幫你？", "voice": "zh-CN-default"}

// 配置
{"type": "config", "asr_language": "zh", "tts_voice": "zh-CN-default"}
```

### 服務器 → 用戶端

```json
// ASR 中間結果
{"type": "asr_interim", "text": "你好我有...", "is_final": false}

// ASR 最終結果
{"type": "asr_final", "text": "你好，我有個問題。", "confidence": 0.95}

// TTS 音訊流
{"type": "tts_audio", "data": "<base64 pcm_chunk>", "format": "pcm16", "sample_rate": 24000}

// TTS 結束
{"type": "tts_end"}

// VAD 狀態
{"type": "vad", "state": "speech|silence"}

// 錯誤
{"type": "error", "code": "xxx", "message": "..."}
```

## 6. 風險與緩解

| 風險 | 機率 | 影響 | 緩解 |
|---|---|---|---|
| sherpa-onnx aarch64 prebuilt lib 不相容 | 低 | 高 | 備用方案：在 RK3568 上原生編譯或使用 Docker |
| RK3568 CPU 不夠跑串流 ASR | 中 | 中 | 先測試 Zipformer 最小模型；可啟用 NPU (RKNN) 路徑 |
| 記憶體不足 (2GB 版) | 中 | 高 | 明確要求 4GB+ 版；使用 INT8 量化模型 |
| WebSocket 二進制音訊效能瓶頸 | 低 | 中 | 使用 tokio task pool；避免大 chunk 拷貝 |
| 模型授權問題 | 低 | 低 | 只使用 Apache 2.0 / MIT 授權模型 |

## 7. 非目標（明確不做的）

- ❌ 不從零寫 ASR/TTS 引擎
- ❌ 不支援 GPU/CUDA（RK3568 無 CUDA）
- ❌ 不支援 HTTP REST（只做 WebSocket）
- ❌ 不做模型訓練或微調
- ❌ 不支援說話人辨識/日誌化（未來可加）
- ❌ 不支援語音增強/降噪（未來可加）
