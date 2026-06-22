# Acceptance Report — RK3568 Rust 語音服務器

> 本報告記錄編譯驗證結果。

---

## 編譯驗證 (2026-06-22)

| 項目 | 結果 | 備註 |
|------|------|------|
| `cargo check` | ✅ 通過 | 0 errors, 10 warnings (均為 unused/dead code) |
| `cargo build --release` | ✅ 通過 | ~9 分鐘完整編譯 |
| Binary size | ✅ **25 MB** | 低於 spec 限制 50 MB |
| Cross-compile to aarch64 | ✅ **原生編譯** (aarch64) | RK3568 Armbian 上原生編譯成功 |
| Compiler | rustc 1.96.0 (2026-05-28) | `aarch64-unknown-linux-gnu` |
| Binary type | ELF 64-bit LSB pie executable, ARM aarch64 | 動態連結 (libstdc++, libm, libc) |

## 專案驗證腳本 (2026-06-22)

| 項目 | 結果 | 備註 |
|------|------|------|
| `node scripts/check.mjs` | ✅ 通過 | 62/62 checks, 1 warning (新 scripts/ 目錄) |
| `node scripts/download-models.mjs --list` | ✅ 通過 | 列出 6 組可用模型 |

## 架構修正 (2026-06-22)

| 項目 | 變更 | 原因 |
|------|------|------|
| VAD engine per-connection | ✅ 修正完成 | 原先全局共享 `VoiceActivityDetector` 在多路連線時會造成語音偵測互相干擾。改為每連線建立專屬 instance，使用 `state.config.vad` 作為 factory。 |

## 新增檔案

| 檔案 | 說明 |
|------|------|
| `scripts/check.mjs` | V4.1 專案驗證腳本 (62 項檢查) |
| `scripts/download-models.mjs` | 模型下載輔助工具 (支援 --list/--asr/--tts/--vad/--all) |

---

## A0：環境準備

| 編號 | 檢查項目 | 狀態 | 備註 |
|------|---------|------|------|
| A0.1 | Cross-compilation / 原生編譯 | ✅ | RK3568 (aarch64) 原生編譯成功，rustc 1.96.0 |
| A0.2 | Binary < 50 MB | ✅ **25 MB** | 遠低於 50 MB 限制 |
| A0.3 | 最小範例在 RK3568 執行 | 🔄 | 需下載模型後實測 |

---

## A1~A6：功能驗收

所有功能驗收項目 (A1.1-A6.7) 均需要：
1. 在 RK3568 上實際運行
2. 實際載入模型檔案
3. 測試工具 (WebSocket client)

**目前狀態**: 實作完成，等待實機測試。

---

## 驗收結論

| 等級 | 結果 |
|------|------|
| ✅ 通過 | ⏳ 待實機驗證 |
| ⚠️ 有條件通過 | — |
| ❌ 不通過 | — |

---

## 測試環境記錄

| 項目 | 內容 |
|------|------|
| 測試日期 | 2026-06-22 (code complete, pending hardware) |
| 開發環境 | Windows x86_64, Rust 1.94.1 |
| 目標平台 | RK3568, Armbian Linux, aarch64 |
| 模型 | 待下載 |
