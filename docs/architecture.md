**繁體中文** | [English](architecture.en.md)

# 架構

DDD＋Clean Architecture，以 Modular Monolith 落地：單一程式、每個業務模組一個 crate、模組內分層。採循序漸進：DDD 戰術模式只用在複雜模組。選型與導入時機見 [decisions.md](decisions.md)。

## 1. 初始目錄規劃
Skeleton 時依實際情況微調；調整後更新本節。

```text
crates/
  core/            # 共用核心：設定、模組註冊與生命週期、錯誤碼、事件 envelope、記憶體事件廣播、outbox（含自己的 migration）
  integrations/    # Twitch（EventSub）、OBS（obws）Adapter：外部事件轉內部事件，處理 OAuth、重連、退避、限流
  modules/
    chat/          # 聊天室顯示（薄）
    alerts/        # 追隨／訂閱特效（薄）
    redemption/    # 兌換紀錄（複雜）
    checkin/       # 打卡簽到（複雜）：訂閱 RedemptionRecorded 與聊天指令事件
    sound/         # 音效管理（薄）
  host/            # 組合根：組裝模組、Axum HTTP／WebSocket 路由、Overlay 靜態頁
apps/
  desktop/         # Tauri 2 殼：同程序啟動 Host、系統匣、單一實例鎖；管理視窗載入 Host 網址
  web/             # Vue 3 + TS：管理 UI、Overlay 頁面、port 修改小頁面
```

## 2. 模組內分層
每個模組 crate 內都有三層資料夾，薄模組也一樣，升級時不必搬檔。

| 層 | 放什麼 | 可依賴 |
|---|---|---|
| `domain/` | 實體、規則；複雜模組用 Aggregate／Value Object／Domain Event | 只有 `core` 的共用型別；**不得**引用 `sqlx`、`axum` |
| `application/` | Use case、port（Repository trait、對外公開的 service trait） | `domain` |
| `infrastructure/` | `db/`：SQLx Repository 實作與 Row；`http/`：Axum handler 與 DTO（產生 OpenAPI）；事件轉換 | `application`、`domain` |

- 模組的 `lib.rs` 只公開 application service trait、對外 DTO 與對外事件；其餘一律 `pub(crate)`。
- 複雜模組：Domain、DTO、Row、對外 Event 各自獨立型別；`domain` 不帶 `serde`／`sqlx` derive，在邊界寫 mapping。
- 薄模組：`domain` 只放 struct 與簡單驗證，不用 Aggregate／Value Object；Row 可與模型共用，只另外分出 DTO。

## 3. 複雜模組與薄模組
以下四題任一答「是」就是**複雜模組**，使用 DDD 戰術模式：
1. 有永遠必須成立的規則嗎？（例：同一兌換 ID 只存一次、同一觀眾同一營運日只能打卡一次）
2. 有狀態轉換，且某些轉換不合法嗎？（例：兌換狀態照 Twitch，從 `UNFULFILLED` 變成 `FULFILLED` 或 `CANCELED`（`CANCELED` 即退還點數）；`CANCELED` 不能再變回有效）
3. 出錯會損害資料或觀眾信任，而不只是畫面錯嗎？
4. 其他模組依賴這些規則，或規則要在多處一致套用嗎？

四題都答「否」就是**薄模組**：設定 CRUD，或把事件轉成畫面與聲音。薄模組一出現第一條上述規則就升級成複雜模組（補齊 domain 模型）；不做降級。

| 模組 | 分類 | 理由 |
|---|---|---|
| 兌換紀錄 | 複雜 | 去重、`UNFULFILLED`／`FULFILLED`／`CANCELED` 狀態轉換、紀錄可能不完整的標記 |
| 打卡簽到 | 複雜 | 每日一次、營運時區、連續天數、三種觸發來源、匯入去重 |
| 個人徽章（MVP 後） | 薄 | 初版只有「觀眾／身分組 → 徽章」；出現多規則優先順序時升級 |
| 事件特效 | 薄 | 佇列、優先順序、重播屬於播放邏輯，放在 application 層 |
| 聊天室顯示、音效管理、SO 剪輯、設定 | 薄 | CRUD 或渲染，沒有資料規則 |

## 4. 模組之間
- **同步查詢**：呼叫對方公開的 application service trait；不得引用對方內部型別。
- **兩種事件**：
  - 整合事件與畫面事件（聊天訊息、模擬事件、連線狀態等）只在記憶體廣播，不寫入資料庫。
  - 模組的 domain event（例：`RedemptionRecorded`）走 outbox，由關心的模組訂閱。
- **Outbox**（兌換紀錄切片建立）：
  - 事件與業務資料寫在同一個交易。
  - dispatcher 從 outbox 取出分派，失敗會重試並在 UI 顯示，重啟後補送。
  - 每個訂閱者各自追蹤投遞狀態，一個訂閱者失敗不會重跑或卡住其他訂閱者。
  - 訂閱者必須可重複執行：處理過的事件 ID 記在自己的 inbox 表，且與業務效果寫在同一個交易。
  - outbox 表屬於 `core`，`core` 有自己的 migration 目錄；寫入 outbox 時沿用呼叫端的交易。
  - 發布事件時就記下當下有哪些訂閱者（含停用中的模組）；之後新增的模組不會收到過去的事件。
  - 訂閱者依事件的 `occurredAt` 判斷業務時間（例如打卡的營運日），不用處理時間，補送的事件才不會算到錯的日子。
  - 重試有上限；超過就把事件移入 dead-letter，在 UI 顯示並提供「重試」或「略過」，避免單一事件永久卡住該訂閱者。
  - 所有訂閱者都投遞完成的事件定期清除；已移除模組的待投遞紀錄一併清除。
- **停用模組**：
  - Skeleton 起就有啟用／停用開關；停用期間記憶體事件（聊天等）直接丟棄，不保留。
  - Outbox 建立後（兌換紀錄切片），停用時由實況主選擇：
    - 「暫停」：事件保留在 outbox 不投遞，重新啟用後依序補送；暫停期間不累計重試次數；UI 顯示積壓數量與最舊事件的時間。
    - 「關閉」：停用期間的事件對該模組標為略過，重新啟用後不補；選擇前先警告，列出會受影響的事件與模組。
    - 從「暫停」改為「關閉」時，尚未投遞的事件一併標為略過。
- **資料**：單一 SQLite 檔；每個模組有自己的資料表（加模組前綴）與 migration 目錄；**禁止跨模組 join**，要別人的資料走公開介面。寫入交易一律用 `BEGIN IMMEDIATE`（或單一 writer 連線），避免 SQLite 回 `SQLITE_BUSY`。
- **Migration**：檔名用時間戳版本，啟動時由 `host` 合併成單一 Migrator 執行。規則：
  - 版本必須全域唯一，合併後依版本排序；有測試檢查。
  - 發布後的 migration 檔不得修改（SQLx 會比對 checksum，連換行符號改變都會導致啟動失敗）。
  - 停用中的模組仍要提供自己的 migration，否則啟動會因版本缺漏失敗；模組即使移除，它的 migration 檔也要保留。
  - 提供一個開發指令，套用合併後的 migration，供 SQLx offline 資料（`.sqlx/`）使用。
- 若出現循環依賴，把共用的事件或型別移到 `core`，不要讓兩個模組互相依賴。
- 外部平台（Twitch、OBS）只出現在 `integrations`；業務模組不依賴外部 SDK，只接收內部事件。外部值在整合層轉換，例如 EventSub 的兌換狀態是小寫（`unfulfilled`／`fulfilled`／`canceled`，另有 `unknown`），Helix API 是大寫，一律轉成內部狀態並處理 `unknown`。

## 5. 對外通訊
- MVP 的用戶端只有管理 UI（Tauri 視窗）與 OBS Overlay，一律走 HTTP＋WebSocket（例外：下方「Tauri IPC 權限」列出的 IPC 指令），只綁 `127.0.0.1`。OBS Dock 與手機瀏覽器控制在 MVP 之後。
- HTTP 做指令與查詢；WebSocket 推事件；用戶端連線先取快照再訂閱，重連後重新取快照。
- WebSocket 事件首版就用固定 envelope：`id`、`type`、`version`、`seq`、`occurredAt`、`source`。內部 domain event 不直接外洩，在 `infrastructure` 轉成對外事件。
- API 契約 code-first：從 Rust 產生 OpenAPI 與前端 TS 型別，CI 檢查產物是否最新。
- **Port**：固定預設 port（數字在 skeleton 時選定）。被佔用時不自動換 port，避免 OBS 網址悄悄失效。此時 Host 沒有啟動，設定頁也打不開，因此改開 port 修改小頁面（由 Tauri 直接提供，見下方「前端檔案」）：顯示錯誤、讓使用者輸入並驗證新 port、儲存後重試綁定。這個頁面的字串同樣走資源鍵。
- **前端檔案**：
  - `apps/web` 有三類 entry：管理 UI、各 Overlay、port 修改小頁面。
  - 管理 UI 與 Overlay 的建置產物用 `rust-embed` 內嵌進執行檔，由 Host 提供。
  - port 修改小頁面建置到獨立目錄，作為 Tauri 的 `frontendDist`（只含這個頁面）。
  - Tauri 設 `app.windows: []`，設定檔不定義任何視窗；視窗都由 Rust 建立（在 `setup` 中，或 port 重試成功後）。
  - 「dev」＝沒有開啟 `tauri/custom-protocol` feature（`cfg(dev)`／`tauri::is_dev()`），不是 `debug_assertions`。`tauri build`（含 `--debug`）會開啟它，是唯一可以散布的建置方式；`tauri dev` 與沒有加這個 feature 的一般 cargo 指令（`cargo run`、`cargo build --release`、`cargo test`、`cargo clippy`）都是 dev。CI 手動加上這個 feature 只用來檢查，不散布產物。
  - `cfg(dev)` 只在 `apps/desktop` 有效：由 `apps/desktop` 判斷是否為 dev，再把 dev 額外允許的 Vite origin 當成執行期設定傳給 Host；`crates/host` 不讀任何 cfg。
  - `rust-embed` 啟用 `debug-embed` feature，debug 建置也真的把檔案內嵌進執行檔。
  - 使用者自訂的 HTML／CSS 樣板存在使用者資料目錄。
- **開發模式**：前端用 Vite dev server 熱更新，`/api` 與 `/ws` proxy 到 Host：
  - proxy 目標寫 `http://127.0.0.1:<port>`，不寫 `localhost`（Node 可能解析成 IPv6 `::1`，但 Host 只綁 IPv4）。
  - proxy 設 `changeOrigin: true`（只改寫 Host 標頭，不改 Origin）與 `ws: true`。
  - Tauri 的 `build.devUrl` 設成 Vite 網址，開發時管理視窗與 port 修改小頁面都由 Vite 提供，並被當成本機 origin；Vite 設 `server.strictPort: true`，避免 port 漂移而對不上白名單。
  - Vite 以 `server.headers` 送出與 `/admin` 相同的 CSP，讓違規在開發時就出現。
  - dev 模式下，Vite 除了 proxy 的 `/api`、`/ws` 以外的回應都允許 `null` origin（`server.cors: false` 加一個 `configureServer` middleware），因為預覽 iframe 要從 `/@vite/client`、`/src/...` 等路徑載入模組。

### 本機 API 安全
只綁 `127.0.0.1` 擋不住瀏覽器裡的惡意網頁（跨站請求、DNS rebinding、WebSocket 劫持），因此另外做：
- **管理視窗**：載入 `http://127.0.0.1:<port>/admin`，在 Host 綁好 port 之後才建立；開發時改載入 Vite dev server。管理視窗與 API 同源，因此不開 CORS，也不必處理 Tauri 的 origin。管理視窗只能導向 app 自己 origin 下的 `/admin` 路徑（`on_navigation` 限制），開新視窗（`window.open`、`target=_blank`）一律拒絕（`on_new_window`）；Device Code Flow 的驗證網址由 Rust 指令用 `tauri-plugin-opener` 在系統瀏覽器開啟，視窗內只顯示代碼。
  - Host 對 `/admin`（含其下所有路徑）送出嚴格的 `Content-Security-Policy`：`script-src 'self'`（禁止 inline script 與 eval）；`connect-src 'self' ipc: http://ipc.localhost`（Tauri IPC）；`style-src 'self' 'unsafe-inline'`（PrimeVue 執行期插入樣式，或改用 PrimeVue 的 nonce）；`img-src 'self' data:` 與 `font-src 'self' data:`（Vite 會把小檔案內嵌成 `data:`）。不設 `default-src`。vue-i18n 使用預先編譯的訊息，不需要 `unsafe-eval`。
- **管理 token**：Host 每次啟動產生隨機 token，不寫入磁碟。管理視窗啟動時透過 Tauri IPC 指令取得；token 只放記憶體，不存 localStorage（WebView2 會寫到磁碟）。管理 HTTP 請求帶 `Authorization` header；管理 WebSocket 連線後第一則訊息帶 token。
- **Tauri IPC 權限**：app 指令在 `build.rs` 用 `tauri_build::AppManifest` 宣告，每個指令都要明確授權。共三組 capability：
  1. port 修改小頁面（本機 origin，只限 `port` 視窗）：只授權「驗證並儲存 port、重試綁定」的指令。
  2. 管理視窗 dev 版（經 `devUrl` 為本機 origin，只限 `admin` 視窗）：授權取得 token 等管理指令。
  3. 管理視窗正式版（Host 網址為遠端 origin）：啟動時用 `Manager::add_capability` 建立，URL 限 `http://127.0.0.1:<port>/admin*`，只限 `admin` 視窗。這組不放在會自動啟用的 `capabilities/` 目錄；保留 Tauri 的 `dynamic-acl` feature（預設開啟）。
- **使用者樣板**：
  - 自訂樣板與 Overlay 絕不在管理視窗裡直接渲染；預覽放在只設 `sandbox="allow-scripts"`（不帶 `allow-same-origin`）的 iframe，樣板裡的程式拿不到管理視窗的 IPC 與 token；有測試確認預覽 iframe 呼叫 `invoke` 會被拒絕。
  - 預覽 iframe 的 origin 是 `null`，無法連 Host；快照與範例事件由管理視窗用 `postMessage` 傳入。
  - Overlay 與樣板路徑的所有回應（含 HTML 與 SVG）都送 Overlay CSP：`script-src 'self'; object-src 'none'`；樣板目錄只提供非腳本類型的檔案（白名單：HTML、CSS、圖片、字型、音效），並加 `X-Content-Type-Options: nosniff`，落實初版只開放 HTML／CSS。
- **Host 標頭**：只接受 `127.0.0.1:<port>` 與 `localhost:<port>`，擋 DNS rebinding。
- **Origin**：所有 WebSocket 連線（含 Overlay）與會改變狀態的請求都檢查 Origin，只接受 `http://127.0.0.1:<port>` 與 `http://localhost:<port>`（目前的 port）；dev 模式另外接受 Vite dev server 的 origin（由 `apps/desktop` 以執行期設定傳入）。沒有 Origin 或 `Origin: null` 的 WebSocket 連線一律拒絕。
- **CORS**：`/api`、`/ws`、`/admin` 不開。只有建置產物的 `assets/` 目錄（管理 UI 與 Overlay 共用 chunk）與樣板目錄送 `Access-Control-Allow-Origin: *`（不帶憑證，內容都是公開程式碼與使用者樣板），因為預覽 iframe 的 origin 是 `null`，Vite 產出的 module script、CSS 與字型都要經 CORS 才能載入。
- **Overlay**：路由唯讀，只能讀快照與訂閱公開事件，沒有任何會改變狀態的端點；OBS 網址不含 token。
- **OBS Dock**：MVP 不做；之後與手機遠端控制一起設計配對方式。

## 6. Host 生命週期
Host 與 Tauri 同程序。關閉管理視窗只隱藏到系統匣，Overlay 繼續運作；從系統匣結束才停止。

啟動順序：
1. 先註冊 `tauri-plugin-single-instance`，確認是唯一實例，避免第二個實例在交棒前去搶 port 而失敗。
2. Host 綁 port；失敗時開啟 port 修改小頁面，使用者儲存後重試。
3. 正式版：為目前的 port 建立 remote capability，再建立管理視窗並載入 Host 網址。dev 模式：管理視窗載入 Vite dev server，使用 capability 2。

## 7. 規則怎麼強制
- 跨模組依賴：由各 crate 的 `Cargo.toml` 強制。
- 層內規則：架構測試在 CI 執行，至少檢查 `domain/` 內沒有任何 `sqlx`／`axum` 字樣（含 `use`、derive、完整路徑）、模組不引用其他模組的非公開型別。
