# Traceability Matrix — RK3568 Rust 語音服務器

> 從需求到實作的雙向追溯。
> 每項實作應可追溯到 spec 需求，每個 spec 需求應有對應實作。

---

## 正向追溯：需求 → 實作

| 需求 ID | 需求描述 | 實作檔案 | 狀態 |
|---------|---------|---------|------|
| F1.1 | WebSocket 傳送 PCM → 即時回傳辨識文字 | `src/ws/handler.rs`, `src/asr/engine.rs` | ✅ 實作 |
| F1.2 | 16 kHz, 16-bit, 單聲道 PCM | `src/asr/engine.rs` (sample_rate=16000, i16→f32) | ✅ 實作 |
| F1.3 | 中間結果 (interim) | `src/ws/handler.rs` (process_audio_chunk → asr_interim) | ✅ 實作 |
| F1.4 | 最終結果含信心度 | `src/ws/handler.rs` (final_result with confidence) | ✅ 實作 |
| F1.5 | 多模型支援 | `src/asr/engine.rs` (zipformer/paraformer/sensevoice/nemo_ctc) | ✅ 實作 |
| F1.7 | ASR 模型切換 (runtime) | `src/ws/handler.rs` (ClientMessage::Config placeholder) | 🔄 部分 |
| F2.1 | Silero VAD | `src/asr/vad.rs` (VadEngine) | ✅ 實作 |
| F2.2 | VAD 狀態回傳 | `src/ws/handler.rs` (vad_speech/vad_silence) | ✅ 實作 |
| F2.4 | VAD 自動觸發 ASR 最終結果 | `src/ws/handler.rs` (silence → finalize) | ✅ 實作 |
| F3.1 | TTS 文字→PCM | `src/tts/engine.rs` (TtsEngine::synthesize) | ✅ 實作 |
| F3.3 | Chunked streaming 回傳 | `src/ws/handler.rs` (TTS chunks via mpsc) | ✅ 實作 |
| F3.5 | 24 kHz, 16-bit, 單聲道 PCM | `src/ws/protocol.rs` (TTS 預設格式) | ✅ 實作 |
| F4.1 | 多客戶端同時連線 | `src/ws/handler.rs` (per-connection task) | ✅ 實作 |
| F4.2 | 獨立 session | `src/ws/handler.rs` (per-connection asr_stream) | ✅ 實作 |
| F4.3 | systemd service | `deploy/voice-server.service` | ✅ 實作 |
| F4.4 | TOML 設定檔 | `src/config.rs`, `config.toml` | ✅ 實作 |
| F4.5 | 存取日誌 | `src/main.rs` (tracing) | ✅ 實作 |
| NFR9 | WebSocket 訊息格式 | `src/ws/protocol.rs` (JSON + binary frames) | ✅ 實作 |
| NFR10 | 離線運行 | No external API calls | ✅ 符合 |

---

## 逆向追溯：實作 → 需求

| 實作檔案 | 對應需求 |
|---------|---------|
| `src/config.rs` | F4.4 |
| `src/asr/engine.rs` | F1.1-F1.5, F1.7 |
| `src/asr/vad.rs` | F2.1-F2.4 |
| `src/tts/engine.rs` | F3.1-F3.5 |
| `src/ws/protocol.rs` | NFR9, F1.1, F3.3 |
| `src/ws/handler.rs` | F1.1, F1.3, F1.4, F2.2, F2.4, F3.3, F4.1, F4.2 |
| `src/error.rs` | (支援) |
| `src/main.rs` | F4.4, F4.5, P2-1 |
| `config.toml` | F4.4 |
| `deploy/install.sh` | P4-1 |
| `deploy/voice-server.service` | F4.3, P4-2 |

---

## 未實作項目 (已知缺口)

| 需求 ID | 原因 | 預計 |
|---------|------|------|
| F1.6 (中英文) | 由模型決定，框架已支援 | 模型配置即可 |
| F2.3 (VAD 敏感度配置) | Config struct 已定義 threshold | P2 feature |
| F3.2 (多語音切換) | TTS config 已支援多模型類型 | P1 feature |
| F3.4 (Piper/Kokoro) | Piper 在 v1.13.3 無官方支援；Kokoro 已可設定 | 待 crate 更新 |
| F3.6 (TTS 取消) | 需要 CancellationToken 機制 | P2-5 later |
| P0-1 (Cross-compilation) | 需要在 Linux 環境設定 | 待實機 |
| P0-3 (下載模型) | 需要在 RK3568 操作 | 待實機 |
