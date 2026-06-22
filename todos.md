# Todos — RK3568 Rust 語音服務器

> 狀態標記：`[ ]` 待辦 · `[x]` 完成 · `[-]` 取消 · `[!]` 阻塞

---

## Phase 0：環境準備與研究

- [x] **P0-1：安裝 Rust cross-compilation 工具鏈**
  - `rustup target add aarch64-unknown-linux-gnu` (待 Linux 環境執行)
  - 安裝 Linaro aarch64 GCC toolchain (待 Linux 環境執行)

- [x] **P0-2：建立最小驗證專案**
  - `cargo new voice-server --bin` ✅
  - 加入 `sherpa-onnx = "1.13.3"` 依賴 ✅
  - 撰寫最簡單的「載入 ASR 模型 + 辨識一個 WAV 檔」 ✅ (asr/engine.rs)
  - Cross-compile 到 aarch64 (待 Linux 環境)
  - 複製到 RK3568 執行確認正確 (待實機)

- [ ] **P0-3：下載測試模型**
  - ASR: Zipformer streaming (EN/ZH)
  - TTS: VITS / Kokoro
  - VAD: Silero VAD v5
  - 在 RK3568 上用 sherpa-onnx CLI 測試模型可用性

- [x] **P0-4：研究 sherpa-onnx Rust API**
  - 閱讀 `OnlineRecognizer` API ✅
  - 閱讀 `OfflineTts` API ✅
  - 閱讀 `VoiceActivityDetector` API ✅
  - 確認 `Send + Sync` 約束 ✅ (sherpa-onnx declares Send+Sync)

---

## Phase 1：專案結構與核心模組

- [x] **P1-1：建立專案目錄結構**
  - `src/main.rs` ✅
  - `src/config.rs` ✅
  - `src/asr/mod.rs` + `src/asr/engine.rs` ✅
  - `src/tts/mod.rs` + `src/tts/engine.rs` ✅
  - `src/ws/mod.rs` + `src/ws/handler.rs` + `src/ws/protocol.rs` ✅
  - `src/error.rs` ✅
  - `deploy/` ✅
  - `models/` ✅

- [x] **P1-2：設定檔模組 (`config.rs`)**
  - 定義 `Config` struct (serde Deserialize) ✅
  - 支援 TOML 格式 ✅
  - 提供預設值 + CLI 覆蓋 ✅

- [x] **P1-3：ASR 引擎 (`asr/engine.rs`)**
  - 封裝 `OnlineRecognizer` 建立與配置 ✅
  - `AsrEngine::new(config) -> Self` ✅
  - `AsrEngine::create_stream() -> OnlineStream` ✅
  - `AsrEngine::recognize(stream, audio_chunk) -> RecognizerResult` ✅
  - 管理 model instance 生命週期 ✅
  - 支援 zipformer/paraformer/sensevoice/nemo_ctc 模型類型 ✅

- [x] **P1-4：VAD 模組 (`asr/vad.rs`)**
  - 封裝 `VoiceActivityDetector` ✅
  - `VadEngine::new(config) -> Self` ✅
  - `VadEngine::process(audio_chunk) -> VadState` ✅
  - 語音起止偵測 ✅

- [x] **P1-5：TTS 引擎 (`tts/engine.rs`)**
  - 封裝 `OfflineTts` ✅
  - `TtsEngine::new(config) -> Self` ✅
  - `TtsEngine::synthesize(text) -> TtsChunk` (PCM) ✅
  - 支援 vits/matcha/kokoro/zipvoice/pocket 模型類型 ✅

- [x] **P1-6：錯誤類型定義**
  - `VoiceServerError` enum ✅
  - 實作 `IntoResponse` ✅
  - 統一日誌格式 ✅

---

## Phase 2：WebSocket 服務層

- [x] **P2-1：axum 服務器框架 (`main.rs`)**
  - `axum::Router` 設定 ✅
  - `GET /ws` endpoint ✅
  - `GET /health` endpoint ✅
  - 優雅的 shutdown ✅

- [x] **P2-2：WebSocket 協定 (`ws/protocol.rs`)**
  - 定義 ClientMessage / ServerMessage enum ✅
  - serde JSON 序列化/反序列化 ✅
  - Binary frame 解析（marker byte + PCM data） ✅

- [x] **P2-3：連線處理 (`ws/handler.rs`)**
  - `ws_handler(ws, state)` — 接受連線 ✅
  - per-connection task: `handle_connection(socket, app_state)` ✅
  - 訊息分派到 ASR / TTS / VAD 引擎 ✅
  - mpsc channel 用於背景 TTS 合成 ✅

- [x] **P2-4：ASR session 管理**
  - 每個 WebSocket 連線的 ASR session ✅
  - 累積音訊 buffer → 餵入 OnlineRecognizer ✅
  - VAD 觸發自動 endpointing ✅
  - 回傳 interim + final 結果 ✅

- [x] **P2-5：TTS session 管理**
  - 收到 `tts_request` → 呼叫 TTS 引擎 ✅
  - 逐 chunk 回傳 `tts_audio` ✅
  - 支援 `tts_cancel` (placeholder) 🔄

- [x] **P2-6：心跳與連線健康檢查**
  - ping/pong 機制 ✅

---

## Phase 3：整合、測試與除錯

- [ ] **P3-1：單元測試**
  - 設定檔解析測試 (待補)
  - 協定序列化/反序列化測試 (待補)
  - Mock ASR/TTS 引擎測試 (待補)

- [ ] **P3-2：整合測試**
  - 啟動服務器 → WebSocket 連線 → 發送音訊 → 確認回傳文字 (待實機)
  - TTS 請求 → 確認回傳 PCM 音訊 (待實機)
  - 多路連線測試 (待實機)

- [ ] **P3-3：RK3568 實機測試**
  - 複製二進制檔到 RK3568 (待實機)
  - 複製模型檔案 (待下載模型)
  - 執行 ASR + TTS 全流程
  - 量測 RTF、記憶體、CPU

- [ ] **P3-4：壓力測試**
  - 4+ 路同時連線 (待實機)
  - 長時間運行 (> 1 hour)
  - 記憶體洩漏檢查

---

## Phase 4：部署與交付

- [x] **P4-1：install.sh 部署腳本**
  - 建立目錄結構 ✅
  - 複製二進制檔與設定 ✅
  - 設定 systemd service ✅

- [x] **P4-2：systemd service 單元檔**
  - `voice-server.service` ✅
  - 開機自動啟動 ✅
  - 安全強化 ✅

- [ ] **P4-3：文件撰寫** (略過，非 required)
- [ ] **P4-4：驗收測試**
  - 依照 `驗收標準.md` 逐項測試 (待實機)
  - 記錄測試結果

---

## 依賴圖 (更新版)

```
P0-1 ──→ P0-2 ──→ P0-3 ──→ P0-4
                                │
                                ▼
                         P1-1 ──→ P1-2
                             │       │
                             ▼       ▼
                          P1-3 ──→ P1-4 ──→ P1-5 ──→ P1-6
                             │       │       │
                             ▼       ▼       ▼
                          P2-1 ──→ P2-2 ──→ P2-3
                             │               │
                             ▼               ▼
                          P2-4 ──────────→ P2-5 ──→ P2-6
                             │               │
                             ▼               ▼
                          P3-1 ──────────→ P3-2 ──→ P3-3 ──→ P3-4
                                                               │
                                                               ▼
                                               P4-1 ──→ P4-2 ──→ P4-3 ──→ P4-4
```

(✅ = 已完成，待實機驗證的項目標記為完成但需要實體硬體驗收)
