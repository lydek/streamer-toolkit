**繁體中文** | [English](README.en.md)

# Streamer Toolkit

本機優先的 Twitch 實況互動工具。OBS 自訂聊天室、事件特效、兌換紀錄、打卡簽到、本機音效，不依賴第三方託管服務。

**原則：功能導向、快速迭代。** 一次做一個功能切片，做完就拿去實際直播試用。

**狀態：** MVP 切片都已完成，正在做實機檢查，準備第一個正式版（見 [scope](docs/scope.md)）。

**技術棧：** Rust＋Axum、Tauri 2＋Vue 3＋TypeScript、SQLite＋SQLx。

## 怎麼跑
正式版發佈後，從 GitHub Releases 下載可攜版 zip，解壓縮後執行 `Streamer Toolkit.exe`；SmartScreen 與「解除封鎖」的步驟見 zip 裡的 `使用說明.txt`。從原始碼建置與開發見 [dev](docs/dev.md)。

## 文件
只維護以下文件：下列四份、複雜模組規格 `docs/modules/<模組>.md`，以及給 AI 程式助理的 [AGENTS.md](AGENTS.md)。繁體中文為權威版本，英文版為同名 `.en.md`。

- [docs/scope.md](docs/scope.md)：做什麼、做到哪算完成、不做什麼。
- [docs/architecture.md](docs/architecture.md)：DDD＋Clean Architecture 分層、模組規則、複雜／薄模組判定。
- [docs/decisions.md](docs/decisions.md)：技術選型與導入時機。
- [docs/dev.md](docs/dev.md)：環境、測試底線、安全硬約束。

`docs/archive/` 是早期的規劃草稿，只存在本機、不進版控，僅供參考，不是規格。待辦與想法記在 GitHub Issues。AI 程式助理請先讀 [AGENTS.md](AGENTS.md)。

## 授權
MIT（見 [LICENSE](LICENSE)）。
