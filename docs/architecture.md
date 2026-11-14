**繁體中文** | [English](architecture.en.md)

# 架構

DDD＋Clean Architecture，以 Modular Monolith 落地：單一程式、每個業務模組一個 crate、模組內分層。採循序漸進：DDD 戰術模式只用在複雜模組。選型與導入時機見 [decisions.md](decisions.md)。

## 1. 初始目錄規劃
Skeleton 時依實際情況微調；調整後更新本節。

```text
crates/
  core/            # 共用核心（package 名稱 `toolkit-core`，避開 Rust 內建的 `core`）：設定、模組註冊與啟用開關、內部整合事件、事件 envelope、記憶體事件廣播；之後加 outbox（含自己的 migration）
  integrations/    # Twitch（EventSub）、OBS（obws）Adapter：外部事件轉內部事件，處理 OAuth、重連、退避、限流
  modules/
    chat/          # 聊天室顯示（薄）
    alerts/        # 追隨／訂閱特效（薄）
    redemption/    # 兌換紀錄（複雜）
    checkin/       # 打卡簽到（複雜）：訂閱 RedemptionRecorded 與聊天指令事件
    sound/         # 音效管理（薄）
  host/            # 組合根：組裝模組、Axum HTTP／WebSocket 路由、Overlay 靜態頁
  arch-tests/      # 只放架構測試（見 §7）
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
- 若出現循環相依，把共用的事件或型別移到 `core`，不要讓兩個模組互相依賴。
- 外部平台（Twitch、OBS）只出現在 `integrations`；業務模組不依賴外部 SDK，只接收內部事件。外部值在整合層轉換，例如 EventSub 的兌換狀態是小寫（`unfulfilled`／`fulfilled`／`canceled`，另有 `unknown`），Helix API 是大寫，一律轉成內部狀態並處理 `unknown`。

## 5. 對外通訊
- MVP 的用戶端只有管理 UI（Tauri 視窗）與 OBS Overlay，一律走 HTTP＋WebSocket（例外：下方「Tauri IPC 權限」列出的 IPC 指令），只綁 `127.0.0.1`。OBS Dock 與手機瀏覽器控制在 MVP 之後。
- HTTP 做指令與查詢；WebSocket 推事件。用戶端每次連上 WebSocket，第一則訊息就是快照（Host 先訂閱事件再取快照，兩者之間不漏也不重複），之後的事件依 `seq` 遞增；重連後重新拿到快照。快照的 `payload` 依事件類型列出保留在記憶體的最近事件（例如 `chat.message` 最近 50 則）。連線狀態也是事件：`obs.status` 只保留最新一則。狀態不含任何秘密，所以放在公開的 `/ws/overlay`：管理 UI 與 Overlay 一連上就從快照拿到目前狀態，管理 UI 也可以用 `GET /api/admin/obs` 讀取。
- WebSocket 事件首版就用固定 envelope：`id`、`type`、`version`、`seq`、`occurredAt`、`source`。內部 domain event 不直接外洩，在 `infrastructure` 轉成對外事件。
- API 契約 code-first：從 Rust 產生 OpenAPI 與前端 TS 型別，CI 檢查產物是否最新。
- **Port**：固定預設 port `47810`，存在資料夾的 `settings.json`。被佔用時不自動換 port，避免 OBS 網址悄悄失效；唯一例外是資料夾第一次啟動（還沒有 `settings.json`，也還沒有 OBS 網址）時，預設 port 被佔用就自動挑一個空的 port 並存下來，之後固定使用。此時 Host 沒有啟動，設定頁也打不開，因此改開 port 修改小頁面（由 Tauri 直接提供，見下方「前端檔案」）：顯示錯誤、讓使用者輸入並驗證新 port、儲存後重試綁定。這個頁面的字串同樣走資源鍵。
- **前端檔案**：
  - `apps/web` 有三類 entry：管理 UI、各 Overlay、port 修改小頁面。
  - 管理 UI 與 Overlay 的建置產物用 `rust-embed` 內嵌進執行檔，由 Host 提供。
  - port 修改小頁面建置到獨立目錄，作為 Tauri 的 `frontendDist`（只含這個頁面）。
  - Tauri 設 `app.windows: []`，設定檔不定義任何視窗；視窗都由 Rust 建立（在 `setup` 中，或 port 重試成功後）。
  - 「dev」＝沒有開啟 `tauri/custom-protocol` feature（`cfg(dev)`／`tauri::is_dev()`），不是 `debug_assertions`。`tauri build`（含 `--debug`）會開啟它，是唯一可以散布的建置方式；`tauri dev` 與沒有加這個 feature 的一般 cargo 指令（`cargo run`、`cargo build --release`、`cargo test`、`cargo clippy`）都是 dev。CI 手動加上這個 feature 只用來檢查，不散布產物。
  - `cfg(dev)` 只在 `apps/desktop` 有效：由 `apps/desktop` 判斷是否為 dev，再把 dev 額外允許的 Vite origin 當成執行期設定傳給 Host；`crates/host` 不讀任何 cfg。
  - `rust-embed` 啟用 `debug-embed` feature，debug 建置也真的把檔案內嵌進執行檔。
  - 使用者自訂的 HTML／CSS 樣板存在資料夾的 `templates/`（見下方「本機 API 安全」的「使用者樣板」）。
- **開發模式**：前端用 Vite dev server 熱更新，`/api`、`/ws` 與 `/templates` proxy 到 Host：
  - proxy 目標寫 `http://127.0.0.1:<port>`，不寫 `localhost`（Node 可能解析成 IPv6 `::1`，但 Host 只綁 IPv4）。
  - `<port>` 每個請求都重新讀 dev 資料夾（`target\debug\data\`，有設 `CARGO_TARGET_DIR` 時以它為準）的 `settings.json`，沒有檔案時用預設 port：Vite 比 app 先啟動，資料夾第一次啟動時可能自動換 port，port 修改小頁面也可能存下新的 port。
  - proxy 設 `changeOrigin: true`（只改寫 Host 標頭，不改 Origin）與 `ws: true`。
  - Tauri 的 `build.devUrl` 設成 Vite 網址，開發時管理視窗與 port 修改小頁面都由 Vite 提供，並被當成本機 origin；Vite 設 `server.strictPort: true`，避免 port 漂移而對不上白名單。
  - Vite 以 `server.headers` 送出與 `/admin` 相同的 CSP，`/overlay/` 下的頁面改送 Overlay CSP（`vite.config.ts` 的 middleware），讓違規在開發時就出現。
  - dev 模式下，Vite 除了 proxy 的 `/api`、`/ws`、`/templates` 以外的回應都允許 `null` origin（`server.cors: false` 加一個 `configureServer` middleware），因為預覽 iframe 要從 `/@vite/client`、`/src/...` 等路徑載入模組。

### 本機 API 安全
只綁 `127.0.0.1` 擋不住瀏覽器裡的惡意網頁（跨站請求、DNS rebinding、WebSocket 劫持），因此另外做：
- **管理視窗**：載入 `http://127.0.0.1:<port>/admin`，在 Host 綁好 port 之後才建立；開發時改載入 Vite dev server。管理視窗與 API 同源，因此不開 CORS，也不必處理 Tauri 的 origin。管理視窗只能導向 app 自己 origin 下的 `/admin` 路徑（`on_navigation` 限制），開新視窗（`window.open`、`target=_blank`）一律拒絕（`on_new_window`）；Device Code Flow 的驗證網址由 Rust 指令用 `tauri-plugin-opener` 在系統瀏覽器開啟，視窗內只顯示代碼。其他外部網址也一樣：每個目的地各做一個網址寫死、不接受參數的指令，不做接受網址參數的通用指令（管理視窗若被注入腳本，可用它開任意網址或自訂通訊協定，例如 `ms-settings:`），也不授予任何視窗 opener plugin 的權限。
  - Host 對 `/admin`（含其下所有路徑）送出嚴格的 `Content-Security-Policy`：`script-src 'self'`（禁止 inline script 與 eval）；`connect-src 'self' ipc: http://ipc.localhost`（Tauri IPC）；`style-src 'self' 'unsafe-inline'`（PrimeVue 執行期插入樣式，或改用 PrimeVue 的 nonce）；`img-src 'self' data:` 與 `font-src 'self' data:`（Vite 會把小檔案內嵌成 `data:`）；`frame-src 'none'`（正式版的 IPC capability 涵蓋整個 Host origin，同源頁面不得被框進管理視窗；之後加入樣板預覽時只放行 sandbox iframe 需要的來源）。不設 `default-src`。vue-i18n 使用預先編譯的訊息，不需要 `unsafe-eval`。
- **管理 token**：Host 每次啟動產生隨機 token，不寫入磁碟。管理視窗啟動時透過 Tauri IPC 指令取得；token 只放記憶體，不存 localStorage（WebView2 會寫到磁碟）。管理 HTTP 請求帶 `Authorization` header；管理 WebSocket 連線後第一則訊息帶 token。
- **Tauri IPC 權限**：app 指令在 `build.rs` 用 `tauri_build::AppManifest` 宣告，每個指令都要明確授權。共三組 capability：
  1. port 修改小頁面（本機 origin，只限 `port` 視窗）：只授權「驗證並儲存 port、重試綁定」的指令。
  2. 管理視窗 dev 版（經 `devUrl` 為本機 origin，只限 `admin` 視窗）：授權取得 token 等管理指令。
  3. 管理視窗正式版（Host 網址為遠端 origin）：啟動時用 `Manager::add_capability` 建立，URL 限 `http://127.0.0.1:<port>`，只限 `admin` 視窗（Tauri 用 IPC 請求的 Origin 比對，不含路徑，因此無法限定到 `/admin`；路徑由 `on_navigation` 限制）。這組不放在會自動啟用的 `capabilities/` 目錄；保留 Tauri 的 `dynamic-acl` feature（預設開啟）。
  - 管理視窗（第 2、3 組）除了 app 指令，只另外授權 `core:event:allow-listen`、`core:event:allow-unlisten`（用來接收關閉提示事件），不加 `core:default` 或 `core:window:*`（正式版這組 capability 涵蓋整個 Host origin）。
- **使用者樣板**：
  - 自訂樣板與 Overlay 絕不在管理視窗裡直接渲染；預覽放在只設 `sandbox="allow-scripts"`（不帶 `allow-same-origin`）的 iframe，樣板裡的程式拿不到管理視窗的 IPC 與 token；有測試確認預覽 iframe 呼叫 `invoke` 會被拒絕。
  - 預覽 iframe 的 origin 是 `null`，無法連 Host；快照與範例事件由管理視窗用 `postMessage` 傳入。
  - Overlay 與樣板路徑的所有回應（含 HTML 與 SVG）都送 Overlay CSP：`script-src 'self'; object-src 'none'`；樣板目錄只提供非腳本類型的檔案（白名單：HTML、CSS、圖片、字型、音效），並加 `X-Content-Type-Options: nosniff`，落實初版只開放 HTML／CSS。
  - 聊天室樣板放在資料夾的 `templates/chat/`：`template.html`（HTML 片段）、`style.css`，以及圖片、字型等檔案。這個目錄不存在時（例如第一次啟動）複製內建預設樣板進去，不覆寫既有檔案；預設樣板的原始檔在 `apps/web/src/overlay/chat/default-template/`，同時內嵌進執行檔。
  - 綁定方式：`template.html` 要有一個 `data-chat-list` 元素（新訊息加在它裡面）與一個 `<template data-chat-message>`（一則訊息的標記），裡面用 `data-field="userName"`、`data-field="text"` 標出填值位置；觀眾有設定名稱顏色時，以 CSS 變數 `--user-color` 設在每則訊息上（樣式要寫後備值，例如 `var(--user-color, #fff)`）。Overlay 用 `fetch` 取得 `template.html`、以 `DOMParser` 解析（解析時不執行任何程式），移除 `script`、`iframe`、`object`、`embed`、`base`、`meta` 等元素與 `on*` 屬性。網址屬性（`src`、`href`、`xlink:href`、`poster`、`action`、`formaction`）只保留 `http:`、`https:`、`data:` 與相對路徑，其餘（例如 `javascript:`）移除；相對路徑改指向樣板目錄（以 `/` 開頭的路徑以 Host 根目錄為準，不指向樣板目錄），`#` 開頭的同頁片段（例如 SVG 的 `<use href="#icon">`）不改。`style` 屬性、`<style>` 與 `srcset` 裡的相對路徑不改寫，會以 Overlay 頁面為基準，所以 `url()` 要寫在 `style.css`。新樣板先隱藏掛上，等樣式表載入（最多 3 秒）才換掉舊樣板，避免閃出沒有樣式的畫面。觀眾輸入一律以 `textContent` 寫入，不當 HTML 解析。
  - 樣板路由 `GET /templates/chat/{*path}`：路徑每一段不得為空、`.` 或 `..`，也不得含 `\`、`:`；檔案經 `canonicalize` 後必須仍在樣板目錄內（擋 `..`、符號連結與 junction）；回應另加 `Cache-Control: no-store`。`/templates` 下的標頭由一層 middleware 加上，連進入 handler 前就產生的錯誤回應（編碼錯誤、錯誤的方法、找不到路徑、Host 標頭被拒）也有。
  - 備援：`template.html` 不存在、無法讀取（包括不是 UTF-8）、超過 1 MB，或在 HTML 註解以外缺少 `data-chat-list` 屬性或帶 `data-chat-message` 的 `<template>` 標籤時，樣板路由整組改提供內建預設樣板，`GET /api/admin/templates/chat` 回傳錯誤碼（`template.missing`／`template.unreadable`／`template.invalid`），由管理 UI 顯示。Overlay 取不到樣板或解析不到標記時也改用預設樣板，畫面不會空白。
  - 即時套用：Host 每秒掃描樣板目錄內可提供檔案（白名單內的類型，不含編輯器暫存檔）的路徑、大小與修改時間（輪詢，不另加檔案監看依賴），有變化就重新檢查樣板，並在 `/ws/overlay` 發布 `template.changed` 事件（`source` 為 `system`，不放進快照）。Overlay 收到這個事件或新的快照時重新載入樣板；網址若指向樣板目錄，就加上新的版本參數，避開快取；目前的訊息保留，以新樣板重新畫出。
- **Host 標頭**：只接受 `127.0.0.1:<port>` 與 `localhost:<port>`，擋 DNS rebinding。
- **Origin**：所有 WebSocket 連線（含 Overlay）與會改變狀態的請求都檢查 Origin，只接受 `http://127.0.0.1:<port>` 與 `http://localhost:<port>`（目前的 port）；dev 模式另外接受 Vite dev server 的 origin（由 `apps/desktop` 以執行期設定傳入）。沒有 Origin 或 `Origin: null` 的 WebSocket 連線一律拒絕。
- **CORS**：`/api`、`/ws`、`/admin` 不開。只有建置產物的 `assets/` 目錄（管理 UI 與 Overlay 共用 chunk）與樣板目錄送 `Access-Control-Allow-Origin: *`（不帶憑證，內容都是公開程式碼與使用者樣板），因為預覽 iframe 的 origin 是 `null`，Vite 產出的 module script、CSS 與字型都要經 CORS 才能載入。
- **Overlay**：路由唯讀，只能讀快照與訂閱公開事件，沒有任何會改變狀態的端點；OBS 網址不含 token。
- **OBS Dock**：MVP 不做；之後與手機遠端控制一起設計配對方式。

### Twitch 流量限制
數字以 Twitch 官方文件為準；Twitch 調整時更新這裡。
- **Helix API**：token bucket，每個 Client ID＋使用者一桶，每分鐘補滿（例如 800 點，多數端點每次 1 點）。所有 Helix 請求都經過 `integrations` 裡同一個 Helix client：401 只 refresh 並重試一次；429 依 `Ratelimit-Reset` 等到重置（最多 1 分鐘）再試，不立即重送。有些 429 與點數桶無關（例如 EventSub 上限），沒有 `Ratelimit-Reset` 時改用一般退避。
- **EventSub WebSocket**：每個使用者 token（Client ID＋使用者）最多 3 條有訂閱的連線、每條最多 300 個訂閱、`max_total_cost` 為 10。本工具每個帳號只開 1 條連線，換 Client ID 或登出時先關掉舊連線；收到 `session_reconnect` 時改連 Twitch 給的 reconnect URL（不算新連線，訂閱沿用，不重新訂閱）；連線失敗以指數退避重連（1 秒起，最多 60 秒）。
- **Device Code Flow**：依 Twitch 回傳的 `interval` 輪詢到 `expires_in` 為止，不自行加快；間隔最少 1 秒。
- **聊天發訊**：MVP 沒有發訊功能。之後要發訊（例如打卡回覆）時，必須在本機排隊並限速，採保守值 20 則／30 秒。Twitch 的上限是一般帳號 20 則／30 秒、實況主／mod／VIP 100 則／30 秒（實況主發的訊息也會算進 20 則的桶）；超過時 Twitch 會忽略該帳號的訊息 1 小時。

### OBS 連線
- 用 `obws` 連 OBS 內建的 obs-websocket 5，不開 TLS。位址存在 `settings.json` 的 `obs.host`／`obs.port`（預設 `127.0.0.1`／`4455`）；密碼存 Windows Credential Manager，取出後立即包成 `SecretString`，不寫入 `settings.json` 與 log；沒設密碼就不帶驗證連線（OBS 關閉驗證時這是正常狀態）。交握時只送 challenge 回應，不送密碼本身。
- MVP 只看連線狀態，不控制場景或來源：不訂閱任何 OBS 事件，只從連線關閉得知 OBS 結束。
- 連不上或斷線時以指數退避重連（1 秒起，最多 60 秒，與 EventSub 相同）；連上後重新從 1 秒計算。每次連線嘗試（含交握與版本檢查）最多 10 秒。
- 狀態（事件 `obs.status`，`payload` 為 `state` 與 `reason`；`reason` 是固定代碼，`connected` 與剛啟動時為 `null`）：
  - `connected`：已連上。
  - `reconnecting`：還沒連上，或會自己恢復，例如 OBS 沒開（`obs.unreachable`）、OBS 關閉（`obs.disconnected`）、連上但交握失敗或沒有在時限內完成（`obs.handshake_failed`，例如對方不是 obs-websocket）。OBS 沒開是正常狀態，不算錯誤。
  - `error`：仍會重試，但要使用者處理，例如密碼錯誤（`obs.auth_failed`）、OBS 要求密碼但沒有設定（`obs.password_required`）、版本不支援（`obs.unsupported_version`，需要 OBS 30.2 以上、obs-websocket 5.5 以上的 5.x）。
- log 只記狀態變化，OBS 長時間沒開也不會一直寫 log。
- 管理 API `/api/admin/obs`：
  - `GET` 回傳位址、是否已設密碼（`password_set`，不回傳密碼本身）與目前狀態；讀不到 Credential Manager 時回 500 `obs.secret_store_failed`。
  - `PUT` 修改位址與密碼（`password` 省略表示不變，空字串表示刪除），修改後立即用新設定重連。host 只接受主機名稱與 IPv4、port 不得為 0，否則回 400 `obs.invalid_address`。
  - `PUT` 先存 `settings.json` 再寫密碼：密碼寫入失敗時新位址仍會生效，回 500 `obs.secret_store_failed`；存不了 `settings.json` 時回 500 `settings.save_failed`，位址、密碼與連線都維持原樣。

## 6. Host 生命週期
Host 與 Tauri 同程序。關閉管理視窗只隱藏到系統匣，Overlay 繼續運作；關閉 port 修改小頁面也留在系統匣；從系統匣選「結束」才停止。

按 ✕ 關閉管理視窗時，Rust 一律 `prevent_close()`：`settings.json` 已存「不再提示」就直接隱藏；否則送事件給管理視窗顯示提示，前端用 app 指令 `close_hint` 回報「已顯示」「取消」或「隱藏」，「隱藏」時一併帶「不再提示」的勾選結果，由 Rust 寫入 `settings.json`。2 秒內沒收到「已顯示」，或提示中又按一次 ✕，就直接隱藏，避免按 ✕ 沒反應。

### 資料夾
每個資料夾是一個執行單位，以 `instance-id` 識別：設定、記錄檔、快取與 WebView2 資料都在裡面。程式只發可攜版，資料預設放在執行檔旁的 `data` 資料夾。
- 使用哪個資料夾：命令列 `--data-dir <路徑>` 指定的資料夾；沒有指定就用執行檔旁的 `data` 資料夾（dev 建置也一樣，所以在 `target\debug\data\`）。
- 內容：`settings.json`、`logs/`（JSON，按日輪替，保留 14 天）、`cache/`、`templates/`（使用者樣板）、`WebView2/`、`instance-id`；之後加資料庫與升級前備份。
- 一個資料夾同時只跑一份；不同資料夾可以同時執行（例如測試新版）。`instance-id` 是資料夾裡的隨機 ID，搬移資料夾後不變；複製出來的資料夾被視為同一份，不能和原本的同時執行。
- Twitch 登入與 OBS 密碼依資料夾分開：Credential Manager 的項目名稱是 `<instance-id>/twitch` 與 `<instance-id>/obs`。
- 資料夾無法寫入時顯示錯誤並結束，不改用其他位置。
- 多份並存：解壓縮成多份，或用 `--data-dir` 指定不同資料夾。不出安裝檔（見 decisions.md「散布」）。

啟動順序：
1. 決定資料夾並確認可寫入；`tauri-plugin-single-instance` 依 identifier 命名它的鎖，因此啟動時把 identifier 加上 `instance-id`，讓鎖依資料夾區分。先註冊這個 plugin，第二份程式在交棒前不會去搶 port。
2. Host 綁 port；失敗時開啟 port 修改小頁面，使用者儲存後重試。
3. 正式版：為目前的 port 建立 remote capability，再建立管理視窗並載入 Host 網址。dev 模式：管理視窗載入 Vite dev server，使用 capability 2。
4. 建立任何視窗前先確認有 WebView2 Runtime（Windows 11 內建，少數 Windows 10 沒有）。缺少時顯示說明並開啟微軟的下載頁，Host、Overlay 與系統匣照常運作；安裝後在系統匣選「開啟」就會建立視窗，不必重新啟動。

## 7. 規則怎麼強制
- 跨模組相依性：由各 crate 的 `Cargo.toml` 強制。
- 層內規則：架構測試（`crates/arch-tests`）在 CI 執行，檢查 `domain/` 內沒有任何 `sqlx`／`axum` 字樣（含 `use`、derive、完整路徑），並用 `cargo metadata` 檢查相依方向：模組只能依賴 `core` 與其他模組，不依賴 `integrations` 與 `host`；`core` 不依賴其他 workspace crate；`integrations` 只依賴 `core`；外部平台 SDK（`obws`、`tokio-tungstenite`）只有 `integrations` 可以依賴。模組的內部型別一律 `pub(crate)`，其他模組只看得到對方公開的介面；Cargo 本身禁止循環相依，出現循環時把共用型別移到 `core`。
