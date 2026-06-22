# Rollback Plan — RK3568 Rust 語音服務器

> 記錄每個變更的可逆路徑。

---

## 當前部署狀態

**Phase**: 研發階段 (尚未部署到 RK3568)
**Git**: 尚未初始化 git repo

---

## 變更日誌

| 日期 | 變更 | 回滾方式 |
|------|------|---------|
| 2026-06-22 | 初始專案結構: Cargo.toml, src/*, config.toml | `git clean -fd` 或刪除不想要的檔案 |

---

## 回滾程序

### 1. 專案層級回滾

如果新實作的功能有問題，回滾方式：

```bash
# 如果已初始化 git repo:
git checkout -- <file>        # 單一檔案
git revert HEAD               # 反向 commit

# 如果尚未 git init:
# 使用 backup 複本手動回復
```

### 2. 檔案層級回滾

| 檔案 | 相依性 | 回滾風險 |
|------|--------|---------|
| Cargo.toml | 所有 .rs 檔 | 高 - 影響整個專案 |
| src/config.rs | main.rs, asr/engine.rs, tts/engine.rs, asr/vad.rs | 中 |
| src/asr/engine.rs | ws/handler.rs | 低 - 僅 ASR 功能 |
| src/asr/vad.rs | ws/handler.rs | 低 - 僅 VAD 功能 |
| src/tts/engine.rs | ws/handler.rs | 低 - 僅 TTS 功能 |
| src/ws/protocol.rs | ws/handler.rs | 低 - 僅協定 |
| src/ws/handler.rs | main.rs, 所有引擎 | 中 - 核心邏輯 |
| src/error.rs | 所有模組 | 中 |
| src/main.rs | 所有模組 | 高 - entry point |
| config.toml | 執行時期 | 低 - 可重新產生 |
| deploy/* | 部署環境 | 低 |

### 3. 依賴性回滾

- sherpa-onnx v1.13.3 → 修改 Cargo.toml 版本號
- axum v0.7 → 檢查相容性變更

---

## 關鍵回滾點

1. **Cargo.toml 依賴**: 鎖定版本號，回滾只需改版本號後 `cargo update`
2. **Config 結構**: 向後相容預設值，舊設定檔仍可運作
3. **WS 協定**: 所有訊息 type 有獨特名稱，可版本共存
