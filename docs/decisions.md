**繁體中文** | [English](decisions.en.md)

# 技術決策

以下皆已決定（2026-11-12）。實際遇到問題才改，改的時候在最下方「變更紀錄」寫一行。分層與模組規則見 [architecture.md](architecture.md)。

## 選型
| 項目 | 決策 |
|---|---|
| 架構 | DDD＋Clean Architecture，以 Modular Monolith 落地 |
| DDD 範圍 | 戰術模式只用在複雜模組；判定標準見 architecture.md |
| 模組落地 | Cargo workspace，每個模組一個 crate，內部 `domain`／`application`／`infrastructure` 資料夾；薄模組也保留三層 |
| 模組溝通 | 公開 application service trait＋domain event（走 outbox）；聊天等整合事件只在記憶體廣播 |
| 資料所有權 | 單一 SQLite 檔，資料表歸屬模組，禁止跨模組 join；寫入交易一律 `BEGIN IMMEDIATE`（或單一 writer 連線） |
| 模型分離 | 複雜模組：Domain／DTO／Row／Event 全分離；薄模組：只分出 DTO |
| 後端 | Rust＋Axum |
| 桌面／前端 | Tauri 2＋Vue 3＋TypeScript |
| 資料庫存取 | SQLite＋SQLx，schema 由 migration 管理 |
| 通訊 | 所有用戶端都走 HTTP（指令／查詢）＋WebSocket（事件），快照＋重連 resync；例外是少數 Tauri IPC 指令（見 architecture.md） |
| API 契約 | Code-first：從 Rust 產生 OpenAPI 與 TS 型別 |
| 事件格式 | 首版就用 envelope：`id`、`type`、`version`、`seq`、`occurredAt`、`source` |
| Host 生命週期 | 與 Tauri 同程序，關視窗隱藏到系統匣，單一實例 |
| 架構強制 | crate 相依性＋架構測試（CI 執行） |
| 測試 | 首版：Unit＋Integration（真 SQLite）＋Contract（產物是否最新）；UI E2E 延後 |
| CI | GitHub Actions，skeleton 就建，只跑 Windows；另跑 `cargo clippy --features tauri/custom-protocol` 檢查正式版才編譯的程式碼 |
| 正式發佈 | 一律用 `tauri build`；不得散布沒有開啟 `tauri/custom-protocol` 的建置產物 |
| Overlay 自訂 | 初版只開放 HTML／CSS 樣板 |
| 平台 | Windows 優先；macOS 列入 MVP 後；Linux 不支援 |
| 決策紀錄 | 本檔＋architecture.md，不寫 ADR |
| 文件結構 | README＋`docs/` 四份（scope／architecture／decisions／dev）；複雜模組另寫 `docs/modules/<模組>.md` |
| 文件語言 | README 與 `docs/` 皆雙語：繁中為權威版本，英文版為同名 `.en.md`；平時只寫中文，commit 文件前才產生英文；以 hash 檢查同步（規則見 dev.md）。封存文件不翻譯 |
| 程式語言慣例 | 識別碼、註解、API／DB 欄位、錯誤碼一律英文 |
| Commit | 英文 Conventional Commits，例：`feat(checkin): add daily check-in rule` |
| Backlog | GitHub Issues（label 區分 MVP／之後／bug），不寫在文件裡 |
| AI 指引 | `AGENTS.md`（英文）；`CLAUDE.md` 只引用 `AGENTS.md` |
| 遠端 repo | GitHub public |
| 授權 | MIT（`LICENSE`） |
| 相依套件授權 | Rust（`cargo-deny`）允許清單：MIT、Apache-2.0、BSD-2／3-Clause、ISC、Zlib、Unicode-3.0、MPL-2.0、CDLA-Permissive-2.0。前端另設清單，只檢查會打包進產物的相依套件：上述加上 0BSD、CC0-1.0、CC-BY-4.0、BlueOak-1.0.0、Python-2.0。GPL／LGPL／AGPL 一律擋下 |
| 第三方授權清單 | `cargo-about`（Rust）＋`pnpm licenses`（前端）合成 `THIRD_PARTY_LICENSES`，放進可攜版 zip 與關於畫面 |
| 內建素材 | 只收可再散布的素材（CC0、CC-BY、字型 OFL 等），來源與授權記在 `assets/CREDITS.md`；使用者匯入的素材不進 repo |
| 分支 | 預設 `main`；每個切片開 `feat/<切片>` 短命分支，PR 經 CI 通過後 squash 合併 |
| 換行 | `.gitattributes`：一律 LF；`.ps1`／`.bat`／`.cmd` 用 CRLF；圖片、音效、影片、字型標為 binary |
| Git hook | lefthook（`lefthook.yml`） |
| Commit 身分 | GitHub noreply email，只對本 repo 設定 |
| Twitch OAuth | Device Code Flow（public client，不需要 client secret） |
| Twitch app | 每位使用者自己在 Twitch Console 註冊 app（Public client；表單要求的 Redirect URL 填 `http://localhost`），首次設定時引導並填入 Client ID，存在設定；不內建共用 Client ID |
| Twitch scope | 第一次登入就申請 MVP 全部：`user:read:chat`、`moderator:read:followers`、`channel:read:subscriptions`、`channel:read:redemptions` |
| Token 刷新 | 同時只允許一個 refresh；新 refresh token 先寫進 keyring 再使用新 access token，寫入失敗時新 token 留在記憶體並重試寫入（若程式在重試成功前關閉，下次需重新登入）；refresh 失敗（400／401）就重新走 Device Code Flow，UI 顯示「需重新登入」。Public client 的 refresh token 30 天後過期，長時間沒使用需重新登入 |
| 管理視窗 | 載入 Host 網址 `http://127.0.0.1:<port>/admin`（與 API 同源，不開 CORS）；只能導向 `/admin` 路徑，開新視窗一律拒絕；`/admin` 送嚴格 CSP；Twitch 驗證頁由 Rust 指令用系統瀏覽器開啟；開啟縮放快速鍵（`zoom_hotkeys_enabled(true)`：`Ctrl` 加 `+`／`-`、`Ctrl` 加滾輪）；視窗內容區最小 360×480（寬×高） |
| Tauri IPC 權限 | app 指令以 `AppManifest` 宣告；三組 capability：port 修改小頁面（限 `port` 視窗）、管理視窗 dev 版（本機，限 `admin` 視窗）、管理視窗正式版（遠端，啟動時依實際 port 建立，URL 限 Host origin，路徑由 `on_navigation` 限制）；「dev」＝沒有開啟 `tauri/custom-protocol` feature（`cfg(dev)`）：`tauri build` 會開啟，是唯一可散布的建置；`tauri dev` 與一般 cargo 指令都是 dev；由 `apps/desktop` 判斷後以執行期設定告知 Host |
| 本機 API 安全 | 每次啟動的隨機管理 token，經 IPC 指令交給管理視窗、只放記憶體；Host 標頭白名單；Origin 檢查（拒絕 `Origin: null`）；Overlay 唯讀、不帶 token，Overlay 與樣板路徑的所有回應都送 `script-src 'self'; object-src 'none'`；樣板目錄只提供非腳本檔案並加 `nosniff`；樣板預覽放在 sandbox iframe，資料用 `postMessage` 傳入（細節見 architecture.md） |
| Port | 固定預設 port `47810`；被佔用時不自動換 port，改開 port 修改小頁面讓使用者修改並重試；例外是資料夾第一次啟動時預設 port 被佔用，自動挑空的 port 並存下來 |
| 資料夾 | 每個資料夾是一個執行單位，以 `instance-id` 識別：`--data-dir` 指定的資料夾，沒有指定就用執行檔旁的 `data`；設定、記錄檔、快取、WebView2 資料都在裡面；一個資料夾同時只跑一份，不同資料夾可同時執行；Twitch 登入依資料夾分開；無法寫入時顯示錯誤並結束（細節見 architecture.md §6） |
| 前端檔案 | 管理 UI 與 Overlay 用 `rust-embed` 內嵌進執行檔，由 Host 提供；port 修改小頁面作為 Tauri 的 `frontendDist`；`app.windows: []`，視窗都由 Rust 建立（`setup` 中或 port 重試成功後）；建置產物的 `assets/` 目錄與樣板目錄送 `Access-Control-Allow-Origin: *`，供預覽 iframe 載入 |
| 開發模式 | Vite dev server＋proxy（`/api`、`/ws`，目標 `http://127.0.0.1:<port>`，port 每個請求都從 dev 資料夾的 `settings.json` 讀取，`changeOrigin: true`、`ws: true`）到 Host；Tauri `build.devUrl` 設為 Vite 網址，Vite `server.strictPort: true`；Vite 送出與 `/admin` 相同的 CSP；dev 模式下 Vite 除了 `/api`、`/ws` 以外都允許 `null` origin（`server.cors: false`＋middleware），`server.fs.deny` 另加 Cargo 的 `target` 資料夾（dev 資料夾在裡面，`null` origin 不得讀到）；dev 模式的 Origin 白名單加入 Vite origin |
| 管理 UI 版面 | 功能磚首頁＋抽屜＋完整頁（`/admin/<功能>`），不用側欄。抽屜依序分「動作」「現在」「常改的設定」「開啟完整頁」四段，功能用不到的段省略。功能磚分「直播中」「功能」兩組，使用者可整理（組內排序、移除、加入），清單與順序存在 `settings.json`（「沒存過」與「已清空」要能區分），會一併備份還原 |
| 抽屜元件 | 自製：Vue `Teleport` 在並排的 `<aside>`（視窗 > 1000px）與 modal 的原生 `<dialog>`（≤ 1000px）之間移動同一份內容，不重新掛載；不用 PrimeVue Drawer（只有覆蓋模式）；抽屜內的 PrimeVue 浮層元件設 `appendTo="self"`；換位置前記下焦點與各捲動容器的 `scrollTop`，換完還原 |
| 視覺風格 | 藍紫灰基底（OKLCH 色相 278°、彩度 0.028）＋薰衣草強調色 `#d8b7ff`（以色相 300°、彩度 0.13 為目標，裁切進 sRGB）；平面：不用光暈、漸層、半透明面板，圓角 6px；字型用系統字（Segoe UI＋微軟正黑體），不打包文字字型（PrimeIcons 的圖示字型除外）；MVP 只有深色 |

## 套件
| 用途 | 套件 |
|---|---|
| Domain event 分派 | 自寫 outbox dispatcher：事件與業務資料同一個交易寫入 outbox 表，背景分派、每個訂閱者各自追蹤投遞狀態、失敗重試並顯示在 UI、重啟後補送；停用模組時實況主選「暫停」（事件保留、重新啟用後補送）或「關閉」（先警告，略過停用期間的事件）；重試超過上限移入 dead-letter；投遞完成的事件定期清除；訂閱者以 inbox 表依事件 ID 去重（細節見 architecture.md） |
| Migration | 各模組自己的目錄，檔名用時間戳版本（全域唯一）；啟動時由 host 合併成單一 SQLx Migrator；規則與開發指令見 architecture.md |
| 錯誤處理 | `thiserror`（domain／application，對應錯誤碼）＋`anyhow`（只用在 host 組合根與啟動流程）；錯誤碼格式 `<區域>.<原因>`（例：`twitch.sign_in_required`），API 回 `{"code": …}`，UI 以資源鍵 `errors.<錯誤碼>` 顯示，記錄檔欄位也叫 `code` |
| 時間 | `chrono`＋`chrono-tz` |
| ID | UUID v7（`uuid`）；外部 ID（如 Twitch 兌換 ID）另存欄位 |
| Twitch | 自寫 serde 型別（只定義用到的 Helix、OAuth 與 EventSub 欄位）＋自寫 EventSub WebSocket 連線（`tokio-tungstenite`）；聊天走 EventSub `channel.chat.message` |
| 對外 HTTPS | `hyper-util`＋`hyper-rustls`＋`rustls`（`ring`）＋`rustls-platform-verifier`：憑證走 Windows 憑證庫；EventSub 的 `wss://` 共用同一組 TLS 設定 |
| OBS | `obws`；MVP 就接，先顯示 OBS 連線狀態 |
| 憑證 | `keyring-core`＋`windows-native-keyring-store`（keyring 4 建議 app 直接連結的組合），只在 Windows 編譯；不會落到 mock 儲存 |
| 機密值 | `secrecy`：Token 用 `SecretString` 包起來，永不寫進記錄檔 |
| 記錄檔 | `tracing`＋`tracing-subscriber`（JSON）＋`tracing-appender`（按日輪替，保留 14 天）；寫在資料夾的 `logs/`，`RUST_LOG` 可調整等級；輸出層遮蔽只當備援 |
| 內嵌前端 | `rust-embed`（啟用 `debug-embed`） |
| OpenAPI | `utoipa` |
| 前端 API 型別 | `openapi-typescript`＋`openapi-fetch`；WS 事件型別也放進 OpenAPI components |
| 架構測試 | 自寫 Rust 測試：`cargo_metadata` 檢查 crate 相依性，掃描 `domain/` 內任何 `sqlx`／`axum` 字樣（含 derive 與完整路徑） |
| 前端結構 | `apps/web` 單一 Vite 專案、多入口（管理 UI＋各 Overlay＋port 修改小頁面）；Overlay 不載入 UI 元件庫 |
| UI 元件庫 | PrimeVue 4.5.5（`@primeuix/themes` 2.0.3），Aura preset 固定深色；用 `definePreset` 覆寫 primary 與 surface 色階；版本寫死（不用 `^`／`~`），不升到 PrimeVue 5（改為專有授權） |
| 圖示 | PrimeIcons 7.0.0（版本寫死）；不升到 8（改為專有授權） |
| 前端路由 | vue-router（history 模式，base `/admin/`）；dev 版管理視窗載入 `admin/`（不是 `admin/index.html`，否則路由對不上），Vite 加深層路徑 fallback |
| 前端狀態 | Pinia |
| 前端測試 | Vitest＋Vue Test Utils |
| 前端 lint | ESLint（flat config＋`eslint-plugin-vue`＋`@vue/eslint-config-typescript`），只用正確性規則，不含排版規則 |
| UI 字串 | `vue-i18n`＋`@intlify/unplugin-vue-i18n`：資源檔在建置時預先編譯，執行期不需要 `unsafe-eval` |
| 單一實例 | `tauri-plugin-single-instance` |
| 開啟外部網址 | `tauri-plugin-opener`，只在 Rust 端呼叫，於系統瀏覽器開外部網址：管理視窗要開的每個外部網址（Twitch 驗證頁、新版下載頁等）各做一個網址寫死、不接受參數的指令；缺少 WebView2 時由啟動流程直接開微軟下載頁。不做接受網址參數的通用指令，也不授予任何視窗 opener plugin 的權限 |
| 散布 | 只出可攜版 zip（`tauri build --no-bundle` 產生執行檔，再打包成 zip），不出安裝檔；程式不簽章，說明文件附 SmartScreen「仍要執行」與 zip「解除封鎖」的步驟；更新：啟動時查 GitHub Releases 的最新版本，有新版就在管理 UI 提示，並附按鈕開啟下載頁（網址寫死的指令），使用者自己解壓縮覆蓋；zip 絕不包含 `data` 資料夾，覆蓋時不會動到資料；只能升級，舊版程式遇到較新的資料時拒絕開啟 |
| WebView2 | 使用系統的 Evergreen WebView2 Runtime（Windows 11 內建）；啟動時偵測，缺少就顯示說明並開啟微軟下載頁，Host、Overlay 與系統匣照常運作；不隨附固定版 |
| 開發工具 | `@tauri-apps/cli` 與 `lefthook` 為 pnpm devDependencies，版本由 lockfile 鎖定，不需全域安裝 |
| 套件管理 | Node 24 LTS（寫在 `.nvmrc`）；pnpm 用 `npm i -g pnpm` 安裝一次，之後依 `package.json` 的 `packageManager` 自動切換版本；不依賴 Corepack |

## 導入時機
| 時機 | 項目 |
|---|---|
| **Skeleton 就建立** | 模組啟用／停用開關（停用保留資料；停用期間記憶體事件直接丟棄）、port 被佔用時的修改小頁面、結構化記錄＋錯誤碼、事件模擬器（`source=simulation`）、資料夾規劃（程式與資料分開，見上方「資料夾」）、UI 字串資源鍵（vue-i18n，先只有 `zh-TW`）、a11y 基準（定義見 dev.md）、Token 存 Windows Credential Manager、CI、架構測試、`cargo-deny` 授權與安全公告檢查、本機 API 安全（管理 token、Host 與 Origin 檢查、API 不開 CORS）、Twitch app 註冊引導與 Device Code Flow 登入、Token 自動刷新 |
| **用到的切片再做** | SQLite 與 migration 前自動備份（兌換紀錄切片，第一張資料表）、outbox 與停用時的「暫停／關閉」（兌換紀錄切片）、可攜備份格式 `.stbackup`（備份還原切片）、release workflow 與 `THIRD_PARTY_LICENSES`（可攜版切片） |
| **MVP 之後** | OBS Dock、macOS 安裝檔、PostgreSQL／NAS 儲存（Storage Port）、外掛 SDK、Mutation Testing、第二語系 `en`、遠端／手機控制、可攜版自動更新（下載後驗證 ed25519 簽章，使用者按下才替換）、Overlay 自訂 JS 沙箱、診斷報告匯出、深淺主題／DPI 驗證、UI E2E |
| **不做** | 輕量 Harness（只保留「不得放寬驗收」規則，寫在 dev.md）、微服務、GraphQL／gRPC、WebSocket RPC、通用規則引擎、Linux、原生手機 App |

## 變更紀錄
| 日期 | 改了什麼 | 為什麼 |
|---|---|---|
| 2026-11-13 | Node 26 LTS 改為 Node 24 LTS | 開發機已安裝、支援到 2028-04；pnpm 自管版本不依賴 Corepack，26 的優勢對本專案不重要 |
| 2026-11-13 | Tauri CLI 與 lefthook 改為 pnpm devDependencies | 版本由 lockfile 鎖定，不改動系統 |
| 2026-11-13 | Twitch 型別由 `twitch_api` crate 改為自寫 serde 型別 | MVP 只用到少數事件；少一大串相依套件，欄位與 `unknown` 等值的處理完全自己掌握 |
| 2026-11-13 | 資料目錄改為「資料夾」模式（`--data-dir`、可攜版、每個資料夾一份） | 測試與升級時要多份並存；NSIS 同一產品只能登記一份安裝，所以改用可攜版與 `--data-dir` |
| 2026-11-13 | SQLite 與 migration 前自動備份延到兌換紀錄切片 | skeleton 還沒有任何資料表；到時再用真實資料測試備份 |
| 2026-11-13 | 前端 lint 選 ESLint | Vue SFC 的支援最完整 |
| 2026-11-13 | 錯誤碼統一為 `<區域>.<原因>` | API、UI 資源鍵與記錄檔共用同一個碼 |
| 2026-11-13 | core crate 的 package 名稱定為 `toolkit-core` | `core` 會與 Rust 內建的 crate 撞名 |
| 2026-11-13 | MVP 只出 Windows 可攜版，不出安裝檔；資料固定放在執行檔旁（或 `--data-dir`） | 不購買程式碼簽章，未簽章的安裝檔較容易被防毒與 SmartScreen 擋下；可攜版也符合資料夾模式 |
| 2026-11-13 | 程式不簽章 | 不購買憑證；Azure Artifact Signing 只開放美國與加拿大的個人；接受 SmartScreen 警告，說明文件附操作步驟 |
| 2026-11-13 | 缺少 WebView2 時偵測並引導安裝 | Windows 11 內建、Windows 10 絕大多數已有；固定版 Runtime 不會自動更新安全修補，所以不隨附 |
| 2026-11-13 | 可攜版改為「新版提醒＋手動替換」，自動更新延到 MVP 之後 | Tauri updater 在 Windows 只支援安裝檔；直播中不能讓程式自己換掉，手動替換最安全也最簡單 |
| 2026-11-14 | 新增管理 UI 版面、抽屜元件、視覺風格、前端路由、圖示；UI 元件庫定為 Aura preset（固定深色、`definePreset` 覆寫色階）；開啟外部網址限定為網址寫死的指令；管理視窗開啟縮放快速鍵並設最小尺寸 | UI／UX 規劃定案：側欄＋卡片網格太像通用的後台管理系統，改用功能磚；PrimeVue Drawer 不能並排，所以自製抽屜；能帶網址參數的指令在管理視窗被注入腳本時可開任意網址或自訂通訊協定；縮放快速鍵讓使用者能放大（WCAG 1.4.4） |
| 2026-11-14 | PrimeVue 固定在 4.5.5、`@primeuix/themes` 2.0.3、PrimeIcons 7.0.0（版本寫死） | PrimeVue 5、`@primeuix/themes` 3、PrimeIcons 8 改用專有的 PrimeUI License（需授權金鑰、不得拆出原始程式碼），不在允許清單內；以上是最後的 MIT 版本。v4 不會再有修正，出現安全問題時重新評估 |
| 2026-11-14 | 開發模式的 proxy port 改為每個請求從 dev 資料夾（`target\debug\data\`）的 `settings.json` 讀取；Vite `server.fs.deny` 另加 Cargo 的 `target` 資料夾 | Vite 比 app 先啟動，資料夾第一次啟動可能自動換 port，port 修改小頁面也可能存下新的 port，只在啟動時讀一次會對不上；`null` origin 讀得到 Vite 提供的檔案，而 dev 資料夾（設定、記錄檔、WebView2 資料）在 `target` 裡 |
