# Lessons — RK3568 Rust 語音服務器

> 本文件記錄專案進行中學到的教訓與可重複使用的規則。
> 遵循 TEAM.md 的 lessons 與 RSI 協議，只在有真正可重複使用的洞察時新增。

---

## Lesson #1 — 2026-06-22

**Trigger**: 研究發現 sherpa-onnx 已有官方 Rust crate，不需要自己寫 FFI binding 或從零改寫。

**Lesson**: 在決定「Rust 改寫」之前，先確認 upstream 是否已有官方 Rust 支援。sherpa-onnx 官方 crate 1.13.3 已涵蓋所有主要功能（ASR/TTS/VAD/說話人辨識），並且支援 aarch64 Linux 靜態連結。

**Source**: Voice Server RK3568 專案啟動研究

**Future Rule Candidate**: 對任何有 C/C++ library 的專案，下決定前先查 `crates.io` 和 `docs.rs` 確認官方或社群 Rust binding 的存在與成熟度。

---

## Lesson #2 — 2026-06-22

**Trigger**: 語音服務器的瓶頸不在語言而在模型選擇。Python sherpa-onnx 與 Rust sherpa-onnx 使用同一組 C library，推理速度相同。

**Lesson**: 用 Rust 取代 Python 的主要好處不是「加快推理速度」（推理引擎是同一套 C/C++），而是：
1. 靜態連結 → 單一二進制，無 runtime 依賴
2. 記憶體安全 → 長期運行服務更可靠
3. 非同步原生 → axum + tokio 比 Python asyncio 更輕量
4. 部署極簡 → scp 一個檔案就搞定

**Source**: Voice Server RK3568 專案啟動研究

**Future Rule Candidate**: 在評估 Rust vs Python 時，需要區分「推理引擎語言」與「膠水層語言」兩個層面。

---

## Lesson #3 — 2026-06-22

**Trigger**: sherpa-onnx 的 OnlineRecognizer 不是 `Send + Sync`，這在 tokio 多工環境需要特別處理。

**Lesson**: 不要假設 C binding 的 struct 是 thread-safe。需要：
- 每個 tokio task 使用獨立的 `OnlineRecognizer` instance（but 模型檔案會重複載入）
- 或使用 single-thread 的 `LocalSet` 來處理
- 或共用一個 instance + `tokio::sync::Mutex`（可能造成瓶頸）
- 最佳方案：先在 main thread 初始化，再用 `Arc<Mutex<>>` 或 per-connection instance

**Source**: sherpa-onnx Rust API 原始碼研究

**Future Rule Candidate**: T2+ 專案使用外部 crate 時，需要先確認關鍵 struct 的 thread-safety 標記。

---

## Lesson #4 — 2026-06-22

**Trigger**: 初次實作使用 `sherpa-onnx` v1.13.3 時，基於 spec 和 doc 假設的 API 與實際 crate API 有 43 處編譯錯誤。

**Lesson**: 永遠先 `cargo check` 最小範例來確認 crate 的實際 API 簽名，不要全憑 docs.rs 或 upstream README 的假設。關鍵差異：
- `OnlineRecognizer::new()` → `OnlineRecognizer::create()`
- `accept_waves()` → `accept_waveform()`
- `get_result()` 回傳 `Option<RecognizerResult>` 而非 `String`
- VAD 使用 `detected()` 而非 `is_speech()`
- TTS 無 `generate()` 方法，只有 `generate_with_config()`
- `OnlineModelConfig` 使用 `transducer` 而非 `zipformer2`
- `OfflineTtsModelConfig` 無 `piper` 字段

**Source**: Voice Server RK3568 專案 Phase 1 實作

**Future Rule Candidate**: 使用外部 crate 時，在寫主邏輯前先讀取 crate 原始碼確認 API 簽名（`~/.cargo/registry/src/`），然後才開始撰寫封裝層。

---

## Lesson #5 — 2026-06-22

**Trigger**: 程式碼審查發現 `VoiceActivityDetector` 被放在全域 `Arc<Mutex<VadEngine>>` 中共享，但 VAD 會維護內部狀態（音訊緩衝區、模型激活狀態）。多個連線同時發送音訊時，各連線的語音活動偵測會互相干擾。

**Lesson**: 每個 `VoiceActivityDetector` 實例（以及任何內部狀態可變的 C library wrapper）**不應該**跨連線共享。正確做法：
1. 每個 WebSocket 連線建立專屬的 VAD 實例
2. 使用 `VadConfig` 作為 factory，每次建立時載入 Silero VAD 模型
3. 模型記憶體開銷約 5MB/連線，對於 4-8 路連線可接受（20-40MB）

**Source**: Voice Server RK3568 程式碼審查 (2026-06-22)

**Future Rule Candidate**: 任何封裝 C library 的 Rust wrapper，如果其內部 API 包含「餵入資料 → 更新狀態 → 查詢結果」模式（而非純 stateless function call），則需要在架構層面設計為 per-connection instance 或使用明確的 session/stream 隔離。審查 checklist 應包含：「此 wrapper 的每個 method 是否 thread-safe？是否可安全地在多個 tokio task 間共享？」

---

## Lesson #6 — 2026-06-23 — ASR Streaming: 不要重複餵音訊給 sherpa-onnx OnlineStream

**Trigger**: ASR chunked streaming 測試時發現伺服器永遠不產出 interim/final 結果。診斷後發現 `process_audio_chunk()` 每次呼叫會把 `audio_buffer` 中「所有」累積的音訊重新餵給 `OnlineStream::accept_waveform()`。由於 sherpa-onnx 的 `accept_waveform()` 是累加的，重複餵同一段音訊會讓解碼器收到數倍於實際長度的資料，導致無法正確解碼。

**Lesson**: 任何使用 sherpa-onnx `OnlineStream` 的 streaming 場景，必須追蹤「已餵送的位置（offset）」並只餵送「增量（delta）」音訊。常見錯誤模式：
1. ❌ 每次呼叫都餵送整個累積緩衝區 → 重複餵送，解碼器混亂
2. ✅ 只餵送 `audio_buffer[asr_fed_len..]` → 每個 chunk 只被餵一次

**實作方式**：
```rust
// 維護追蹤變數
let mut asr_fed_len: usize = 0;

// 每次只餵新的部分
if asr_fed_len < audio_buffer.len() {
    let new_samples = &audio_buffer[asr_fed_len..];
    engine.recognize(stream, new_samples);
    asr_fed_len = audio_buffer.len();
}
```

**Source**: Voice Server RK3568, `src/ws/handler.rs` — `process_audio_chunk()` fix

**Future Rule Candidate**: 所有串流處理（audio/video/text streaming）中，如果底層 API 採用累加式（push-based）的資料接收模式，必須在實作 layer 使用 delta-tracking（offset 或 pending queue）來確保每個資料單位只被餵送一次。建議 code review checklist 加入：「此 streaming 路徑是否有重複餵送的風險？」
