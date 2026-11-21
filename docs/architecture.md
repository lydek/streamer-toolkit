**繁體中文** | [English](architecture.en.md)

# 架構

DDD＋Clean Architecture，以 Modular Monolith 落地：單一程式、每個業務模組一個 crate、模組內分層。採循序漸進：DDD 戰術模式只用在複雜模組。選型與導入時機見 [decisions.md](decisions.md)。

## 1. 目錄
目錄結構改變時更新本節。

```text
crates/
  core/            # 共用核心（package 名稱 `toolkit-core`，避開 Rust 內建的 `core`）：設定、模組註冊與啟用開關、聊天指令表、內部整合事件、事件 envelope、記憶體事件廣播、outbox（含自己的 migration）
  integrations/    # Twitch（EventSub）、OBS（obws）Adapter：外部事件轉內部事件，處理 OAuth、重連、退避、限流
  modules/
    chat/          # 聊天室顯示（薄）
    alerts/        # 事件特效：追隨、訂閱、小奇點、揪團（薄）
    redemption/    # 兌換紀錄（複雜）
    checkin/       # 打卡簽到（複雜）：訂閱 RedemptionRecorded 與聊天指令事件
    sound/         # 音效管理（薄）
    bgm/           # 背景音樂：音樂庫、歌單、播放器與場景對應（薄）
    counter/       # 計數器：全域計數、每人計數與訂閱馬拉松計時器（薄）
    overlays/      # 畫面工具：休息中／準備中卡片、警語、內建 Overlay 的外觀與兌換用的媒體庫（薄；模組 ID `screen-tools`）
    stream/        # 直播狀態與場次（薄）
    activity/      # 活動紀錄：追隨、訂閱、小奇點、揪團（薄）
    viewers/       # 觀眾名冊（薄）
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
| 打卡簽到 | 複雜 | 每日一次、營運時區、三種觸發來源、匯入去重 |
| 個人徽章（之後的階段） | 薄 | 初版只有「觀眾／身分組 → 徽章」；出現多規則優先順序時升級 |
| 事件特效 | 薄 | 佇列、優先順序、重播屬於播放邏輯，放在 application 層 |
| 聊天室顯示、音效管理、SO 剪輯、設定 | 薄 | CRUD 或渲染，沒有資料規則 |
| 計數器 | 薄 | 加減與每場歸零；「每場只歸零一次」靠存下的場次 ID，沒有其他規則 |

## 4. 模組之間
- **同步查詢**：呼叫對方公開的 application service trait；不得引用對方內部型別。
- **兩種事件**：
  - 整合事件與畫面事件（聊天訊息、模擬事件、連線狀態等）只在記憶體廣播；要留下紀錄的由模組另存自己的表（例如聊天紀錄、活動紀錄）。
  - 模組的 domain event（例：`RedemptionRecorded`）走 outbox，由關心的模組訂閱。
- **Outbox**（打卡簽到切片建立）：兌換紀錄第一次存下一筆紀錄時，在同一個交易發布 `RedemptionRecorded`（事件類型 `redemption.recorded`，`occurredAt` 是兌換時間）。
  - 事件與業務資料寫在同一個交易。
  - dispatcher 從 outbox 依發布順序取出分派，失敗會重試並在 UI 顯示，重啟後補送。
  - 每個訂閱者各自追蹤投遞狀態，一個訂閱者失敗不會重跑或卡住其他訂閱者。
  - 訂閱者必須可重複執行：處理過的事件 ID 記在自己的 inbox 表，且與業務效果寫在同一個交易。
  - outbox 表屬於 `core`，`core` 有自己的 migration 目錄；寫入 outbox 時沿用呼叫端的交易。
  - 發布事件時就記下當下有哪些訂閱者（含停用中的模組）；之後新增的模組不會收到過去的事件。
  - 訂閱者依事件的 `occurredAt` 判斷業務時間（例如打卡的營運日），不用處理時間，補送的事件才不會算到錯的日子。
  - 重試有上限（共 5 次，間隔 1、2、4、8 秒；重試期間擋住該訂閱者後面的事件）；超過就把事件移入 dead-letter，在 UI 顯示並提供「重試」或「略過」，避免單一事件永久卡住該訂閱者。
  - 所有訂閱者都投遞完成的事件每 10 分鐘清除（啟動時也清一次）；已移除模組的待投遞紀錄一併清除。
- **停用模組**：
  - Skeleton 起就有啟用／停用開關；停用期間記憶體事件（聊天等）直接丟棄，不保留。
  - 有訂閱 outbox 的模組（目前只有打卡簽到）停用時由實況主選擇（設定存在 `settings.json` 的 `disabled_modules` 與 `closed_modules`）：
    - 「暫停」：事件保留在 outbox 不投遞，重新啟用後依序補送；暫停期間不累計重試次數；UI 顯示積壓數量與最舊事件的時間。
    - 「關閉」：停用期間的事件對該模組標為略過，重新啟用後不補；選擇前先警告，列出會受影響的事件與模組。
    - 從「暫停」改為「關閉」時，尚未投遞的事件一併標為略過。
- **資料**：單一 SQLite 檔；每個模組有自己的資料表（加模組前綴）與 migration 目錄；**禁止跨模組 join**，要別人的資料走公開介面。寫入交易一律用 `BEGIN IMMEDIATE`（或單一 writer 連線），避免 SQLite 回 `SQLITE_BUSY`；目前只開一條連線。資料庫檔是資料夾的 `toolkit.db`。
- **Migration**：檔名用時間戳版本，啟動時由 `host` 合併成單一 Migrator 執行。規則：
  - 版本必須全域唯一，合併後依版本排序；有測試檢查。
  - 發布後的 migration 檔不得修改（SQLx 會比對 checksum，連換行符號改變都會導致啟動失敗）。
  - 停用中的模組仍要提供自己的 migration，否則啟動會因版本缺漏失敗；模組即使移除，它的 migration 檔也要保留。
  - 開發指令 `cargo run -p host --example migrate -- <資料夾>` 對該資料夾的 `toolkit.db` 套用合併後的 migration，供 SQLx offline 資料（`.sqlx/`）使用。
  - 升級前備份：資料庫已套用過 migration、又有待套用的 migration 時，先用 SQLite 的 `VACUUM INTO` 備份到資料夾的 `backups/`（`toolkit-<UTC 時間>.db`），不直接複製開啟中的檔案。第一次建立的資料庫與沒有新 migration 時不備份。備份失敗就不套用，顯示錯誤並結束。備份不自動刪除（只在升級時產生）。
  - 資料庫有這一版不認得的 migration（較新版程式的資料）時拒絕開啟，顯示錯誤並結束。
- 若出現循環相依，把共用的事件或型別移到 `core`，不要讓兩個模組互相依賴。
- 外部平台（Twitch、OBS）只出現在 `integrations`；業務模組不依賴外部 SDK，只接收內部事件。外部值在整合層轉換，例如 EventSub 的兌換狀態是小寫（`unfulfilled`／`fulfilled`／`canceled`，另有 `unknown`），Helix API 是大寫，一律轉成內部狀態並處理 `unknown`。

## 5. 對外通訊
- 用戶端目前只有管理 UI（Tauri 視窗）與 OBS Overlay，一律走 HTTP＋WebSocket（例外：下方「Tauri IPC 權限」列出的 IPC 指令），只綁 `127.0.0.1`。OBS Dock 與手機瀏覽器控制之後再設計。
- HTTP 做指令與查詢；WebSocket 推事件。用戶端每次連上 WebSocket，第一則訊息就是快照（Host 先訂閱事件再取快照，兩者之間不漏也不重複），之後的事件依 `seq` 遞增；重連後重新拿到快照。快照的 `payload` 依事件類型列出保留在記憶體的最近事件（例如 `chat.message` 最近 50 則）；特效事件 `alert` 不保留，Overlay 重新載入不會重播；特效一收到就另外發布 `alert.received`（同樣不保留），不論 OBS 裡有沒有特效 Overlay 會播，管理視窗的聊天室「事件」依它列出。連線狀態也是事件：`obs.status` 只保留最新一則；OBS 的場景與輸出狀態 `obs.state` 也只保留最新一則。狀態不含任何秘密，所以放在公開的 `/ws/overlay`：管理 UI 與 Overlay 一連上就從快照拿到目前狀態，管理 UI 也可以用 `GET /api/admin/obs` 讀取。兌換紀錄新增或狀態改變時發布 `redemption.changed`：只帶紀錄 ID，觀眾的文字不經過 `/ws/overlay`，也不放進快照；管理 UI 收到它或新的快照時，重新讀取 `GET /api/admin/redemptions`。打卡紀錄新增時發布 `checkin.changed`，同樣只帶觀眾 ID、不放進快照；管理 UI 收到它或新的快照時重新讀取打卡資料。Overlay 頁面連線時帶 `?kind=chat|alerts|checkin|sound|bgm|counter|marathon|break|warnings`，Host 依種類計算開著的連線數（管理 API `GET /api/admin/overlays`），用來知道 OBS 裡有沒有放這個 Overlay；沒帶或不認得的種類（管理視窗、舊頁面）照樣連線，算在 `other`。
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
  - 使用者自訂的 HTML／CSS 樣板（`custom`）存在資料夾的 `templates/`，由使用者自己放入（見下方「本機 API 安全」的「使用者樣板」）。
- **開發模式**：前端用 Vite dev server 熱更新，`/api`、`/ws`、`/templates`、`/sounds`、`/music`、`/media` 與 `/checkin` proxy 到 Host：
  - proxy 目標寫 `http://127.0.0.1:<port>`，不寫 `localhost`（Node 可能解析成 IPv6 `::1`，但 Host 只綁 IPv4）。
  - `<port>` 每個請求都重新讀 dev 資料夾（`target\debug\data\`，由 `pnpm tauri dev` 使用，不看 `--data-dir`；有設 `CARGO_TARGET_DIR` 時以它為準，須為絕對路徑）的 `settings.json`，沒有檔案或內容不合法時用預設 port：Vite 比 app 先啟動，資料夾第一次啟動時可能自動換 port，port 修改小頁面也可能存下新的 port。
  - proxy 設 `changeOrigin: true`（只改寫 Host 標頭，不改 Origin）與 `ws: true`。
  - Tauri 的 `build.devUrl` 設成 Vite 網址，開發時管理視窗與 port 修改小頁面都由 Vite 提供，並被當成本機 origin；Vite 設 `server.strictPort: true`，避免 port 漂移而對不上白名單。
  - Vite 送出和 Host 相同的 CSP：`/overlay/` 下的頁面送 Overlay CSP，其他送 `/admin` 的 CSP（`frame-src` 依請求的 Host 標頭，放行同一 origin 下的聊天室、特效與集點卡 Overlay 頁；標頭不是單純的主機名稱與 port 時為 `'none'`），讓違規在開發時就出現。CSP 由下一條的 middleware 設定，不用 `server.headers`：Vite 最後才套用它，會蓋掉 Overlay CSP。
  - dev 模式下，Vite 除了 proxy 的 `/api`、`/ws`、`/templates`、`/sounds`、`/music`、`/media`、`/checkin` 以外的回應都允許 `null` origin（`server.cors: false` 加一個 `configureServer` middleware），因為預覽 iframe 要從 `/@vite/client`、`/src/...` 等路徑載入模組。
  - 任何網站的 sandbox iframe 都是 `null` origin，所以依上一條，任何網站都讀得到 Vite 提供的檔案。因此 Vite 的 `server.fs.deny` 在預設清單之外加上 Cargo 的 `target` 資料夾：dev 資料夾（設定、記錄檔、WebView2 資料）在裡面，不能經由 Vite 讀到。

### 本機 API 安全
只綁 `127.0.0.1` 擋不住瀏覽器裡的惡意網頁（跨站請求、DNS rebinding、WebSocket 劫持），因此另外做：
- **管理視窗**：載入 `http://127.0.0.1:<port>/admin`，在 Host 綁好 port 之後才建立；開發時改載入 Vite dev server。管理視窗與 API 同源，因此不開 CORS，也不必處理 Tauri 的 origin。管理視窗只能導向 app 自己 origin（開發時是 devUrl 的 origin）下的 `/admin` 路徑（`on_navigation` 限制），開新視窗（`window.open`、`target=_blank`）一律拒絕（`on_new_window`）；Device Code Flow 的驗證網址由 Rust 指令用 `tauri-plugin-opener` 在系統瀏覽器開啟，視窗內只顯示代碼。其他外部網址也一樣：每個目的地各做一個網址寫死、不接受參數的指令，不做接受網址參數的通用指令（管理視窗若被注入腳本，可用它開任意網址或自訂通訊協定，例如 `ms-settings:`），也不授予任何視窗 opener plugin 的權限。
  - Host 對 `/admin`（含其下所有路徑）送出嚴格的 `Content-Security-Policy`：`script-src 'self'`（禁止 inline script 與 eval）；`connect-src 'self' ipc: http://ipc.localhost`（Tauri IPC）；`style-src 'self' 'unsafe-inline'`（PrimeVue 執行期插入樣式，或改用 PrimeVue 的 nonce）；`img-src 'self' data: https://static-cdn.jtvnw.net` 與 `font-src 'self' data:`（Vite 會把小檔案內嵌成 `data:`；聊天室的觀眾頭像與 Twitch 徽章圖只從 Twitch 的圖片 CDN 載入，Host 存下或轉交的網址都先檢查是否在這個網域）；`frame-src` 只放行 `/overlay/chat/`、`/overlay/alerts/` 與 `/overlay/checkin/` 路徑底下的頁面（聊天室與特效 Overlay 頁）：`http://127.0.0.1:<port>` 與 `http://localhost:<port>` 各加上這兩個路徑（樣板預覽用；`<port>` 是 Host 目前綁定的 port；CSP 的路徑比對不看 query，所以不分是否帶 `?preview`；正式版的 IPC capability 涵蓋整個 Host origin，其他同源頁面不得被框進管理視窗）。不設 `default-src`。vue-i18n 使用預先編譯的訊息，不需要 `unsafe-eval`。
- **管理 token**：Host 每次啟動產生隨機 token，不寫入磁碟。管理視窗啟動時透過 Tauri IPC 指令取得；token 只放記憶體，不存 localStorage（WebView2 會寫到磁碟）。管理 HTTP 請求帶 `Authorization` header；管理 WebSocket 連線後第一則訊息帶 token。
- **Tauri IPC 權限**：app 指令在 `build.rs` 用 `tauri_build::AppManifest` 宣告，每個指令都要明確授權。共三組 capability：
  1. port 修改小頁面（本機 origin，只限 `port` 視窗）：只授權「驗證並儲存 port、重試綁定」的指令。
  2. 管理視窗 dev 版（經 `devUrl` 為本機 origin，只限 `admin` 視窗）：授權取得 token 等管理指令。
  3. 管理視窗正式版（Host 網址為遠端 origin）：啟動時用 `Manager::add_capability` 建立，URL 限 `http://127.0.0.1:<port>`，只限 `admin` 視窗（Tauri 用 IPC 請求的 Origin 比對，不含路徑，因此無法限定到 `/admin`；路徑由 `on_navigation` 限制）。開啟新版下載頁的指令只授權在這組：dev 版不查新版，也就不會出現新版提示。這組不放在會自動啟用的 `capabilities/` 目錄；保留 Tauri 的 `dynamic-acl` feature（預設開啟）。
  - 開啟外部網頁的指令一律不收網址：開啟 Twitch 裝置啟用頁、新版下載頁的指令沒有參數；聊天紀錄頁的「在 Twitch 開啟」只收 Twitch 帳號名稱（小寫英數與 `_`，最多 25 字），組成固定的 `https://www.twitch.tv/popout/<帳號>/chat`，不合格就拒絕。
  - 管理視窗（第 2、3 組）除了 app 指令，只另外授權 `core:event:allow-listen`、`core:event:allow-unlisten`（用來接收關閉提示事件），不加 `core:default` 或 `core:window:*`（正式版這組 capability 涵蓋整個 Host origin）。
- **使用者樣板**：
  - 自訂樣板與 Overlay 絕不在管理視窗裡直接渲染；預覽放在只設 `sandbox="allow-scripts"`（不帶 `allow-same-origin`）的 iframe，樣板裡的程式拿不到管理視窗的 IPC 與 token；有測試確認預覽 iframe 呼叫 `invoke` 會被拒絕。
  - 預覽 iframe 的 origin 是 `null`，無法連 Host；快照與其後的事件由管理視窗用 `postMessage` 傳入。
  - 預覽在聊天室完整頁（`/admin/chat`），就是聊天室 Overlay 頁面加上 `?preview`（`/overlay/chat/?preview`）：不連 WebSocket，只接受來自父視窗、且 origin 等於頁面網址 origin 的訊息，格式與 `/ws/overlay` 相同。管理視窗先送一份快照（最近的聊天；沒有聊天時改送標記 `source=simulation` 的範例訊息），之後轉送自己收到的其他事件；重新連線收到新快照、或預覽頁重新載入時，再送一次快照。完整頁的樣板選擇器為每款內建樣板各放一個預覽（`/overlay/chat/?preview&template=<id>`），預覽頁從內建樣板路由載入那一款；選擇 `custom` 時，完整頁另外顯示資料夾樣板目前無法使用的原因（`GET /api/admin/templates/chat`）。特效完整頁（`/admin/alerts`）用同一個選擇器，預覽是 `/overlay/alerts/?preview&template=<id>`：管理視窗送一份空的快照與一則標記 `source=simulation` 的範例特效（帶留言的續訂），預覽不出聲、反覆播放最近收到的那一則，之後轉送的特效事件會取代範例。
  - Overlay 與樣板路徑的所有回應（含 HTML 與 SVG）都送 Overlay CSP：`script-src 'self'; object-src 'none'`；樣板目錄只提供非腳本類型的檔案（白名單：HTML、CSS、圖片、字型、音效），並加 `X-Content-Type-Options: nosniff`，落實初版只開放 HTML／CSS。
  - 選擇：聊天室 Overlay 顯示的樣板記在 `settings.json` 的 `chat_template`，預設 `default`。內建樣板（`default`、`compact`、`bubble`）的原始檔在 `apps/web/src/overlay/chat/templates/<id>/`，同時內嵌進執行檔，只用 HTML／CSS。管理 UI 用 `PUT /api/admin/templates/chat`（`{ "selected": "<id>" }`）切換，Host 只接受 `default`、`compact`、`bubble`、`custom`（其餘回 400 `template.unknown`；存檔失敗回 500 `settings.save_failed`），存檔後發布 `template.changed`。`custom` 改用資料夾的樣板，管理 UI 沒有入口，要手動改 `settings.json` 並重新啟動程式才生效（Host 只在啟動時讀這個值；在管理 UI 改選內建樣板後，要回到 `custom` 得再改一次；E2E 經 API 切換）；`settings.json` 裡無法辨識的值當成 `default`。
  - 字級：在聊天室完整頁，「Overlay 字級」設定所有樣板的文字大小，記在 `settings.json` 的 `chat_text_size`（px），預設不設定，也就是各樣板自己的大小（`default`、`bubble` 24px，`compact` 20px）。管理 UI 用 `PUT /api/admin/templates/chat/text-size`（`{ "text_size": <16–48 或 null> }`）設定，超出範圍或格式不對回 400 `template.invalid_text_size`，存檔失敗回 500 `settings.save_failed`。Host 存檔後發布 `chat.text_size`（`{ "textSize": <px 或 null> }`），快照只保留最新一則；Overlay 與預覽把它設成頁面的 CSS 變數 `--chat-text-size`，樣板的文字大小讀這個變數，沒設定時用自己的預設值，自訂樣板要自己讀它才會跟著變。`settings.json` 裡超出範圍的值當成沒設定。
  - 自訂樣板（`custom`）放在資料夾的 `templates/chat/`：`template.html`（HTML 片段）、`style.css`，以及圖片、字型等檔案。Host 不會建立或複製這個目錄，由使用者自己放入。
  - 綁定方式：`template.html` 要有一個 `data-chat-list` 元素（新訊息加在它裡面）與一個 `<template data-chat-message>`（一則訊息的標記），裡面用 `data-field="userName"`、`data-field="text"` 標出填值位置；觀眾有設定名稱顏色時，以 CSS 變數 `--user-color` 設在每則訊息上（樣式要寫後備值，例如 `var(--user-color, #fff)`）。Overlay 用 `fetch` 取得 `template.html`、以 `DOMParser` 解析（解析時不執行任何程式），移除 `script`、`iframe`、`object`、`embed`、`base`、`meta` 等元素與 `on*` 屬性。網址屬性（`src`、`href`、`xlink:href`、`poster`、`action`、`formaction`）只保留 `http:`、`https:`、`data:` 與相對路徑，其餘（例如 `javascript:`）移除；相對路徑改指向樣板目錄（以 `/` 開頭的路徑以 Host 根目錄為準，不指向樣板目錄），`#` 開頭的同頁片段（例如 SVG 的 `<use href="#icon">`）不改。`style` 屬性、`<style>` 與 `srcset` 裡的相對路徑不改寫，會以 Overlay 頁面為基準，所以 `url()` 要寫在 `style.css`。新樣板先隱藏掛上，等樣式表載入（最多 3 秒）才換掉舊樣板，避免閃出沒有樣式的畫面。觀眾輸入一律以 `textContent` 寫入，不當 HTML 解析。
  - 樣板路由 `GET /templates/chat/{*path}` 提供目前選擇的樣板；`GET /templates/built-in/chat/{id}/{*path}` 提供任一款內建樣板，給預覽用（`custom` 與未知的 ID 回 404）。兩者都只提供白名單內的類型，路徑每一段不得為空、`.` 或 `..`，也不得含 `\`、`:`；資料夾樣板的檔案經 `canonicalize` 後必須仍在樣板目錄內（擋 `..`、符號連結與 junction）；回應另加 `Cache-Control: no-store`。`/templates` 下的標頭由一層 middleware 加上，連進入 handler 前就產生的錯誤回應（編碼錯誤、錯誤的方法、找不到路徑、Host 標頭被拒）也有。
  - 備援：選擇 `custom` 而 `template.html` 不存在、無法讀取（包括不是 UTF-8）、超過 1 MB，或在 HTML 註解以外缺少 `data-chat-list` 屬性或帶 `data-chat-message` 的 `<template>` 標籤時，樣板路由整組改提供內建預設樣板，`GET /api/admin/templates/chat` 回傳錯誤碼（`template.missing`／`template.unreadable`／`template.invalid`），由管理 UI 顯示。Overlay 取不到樣板或解析不到標記時也改用預設樣板，畫面不會空白。
  - 即時套用：Host 每秒掃描樣板目錄內可提供檔案（白名單內的類型，不含編輯器暫存檔）的路徑、大小與修改時間（輪詢，不另加檔案監看依賴），有變化就重新檢查樣板；選擇 `custom` 時在 `/ws/overlay` 發布 `template.changed` 事件（`source` 為 `system`，不放進快照），選擇內建樣板時不發布。切換樣板也發布同一個事件；`payload.overlay` 是 `chat` 或 `alerts`，每個 Overlay 只處理自己的。Overlay 收到這個事件或新的快照時重新載入樣板；網址若指向樣板目錄，就加上新的版本參數，避開快取；目前的訊息保留，以新樣板重新畫出；特效 Overlay 正在播的那則在新樣板裡播完剩餘時間。
- **特效 Overlay**（`/overlay/alerts/`）：
  - 播放：追隨、新訂閱、續訂、送禮訂閱、小奇點、揪團（來自 Twitch 或模擬器）排進共用的呈現佇列（`core` 的 `presentation`，特效與集點卡共用一條，依收到的順序一次播一則，不設上限）：發布 `alert` 事件後，等它的播放時間（6 秒；續訂與小奇點帶留言時 9 秒）再加 1 秒，才播佇列的下一項；輪到的項目若該種 Overlay 沒有連線（見 WebSocket 的連線數），直接跳過、不等待，模擬器送的除外（管理視窗的預覽仍要收到）；payload 的 `durationMs` 告訴 Overlay 要顯示多久。送禮訂閱時 Twitch 會另外為每位受贈者送一則 `channel.subscribe`（`is_gift`），這些不播，只播送禮那一則。模組停用時，輪到的事件直接丟棄。佇列狀態（正在播的、排隊中的、間隔）以事件 `presentation.queue` 發布，快照只保留最新一則，管理 UI 首頁的佇列區依它顯示並倒數大約要等多久。每個項目帶種類與觀眾（`subject`：特效的種類與觀眾、集點卡的觀眾與印章數、兌換音效的音效名稱與兌換的觀眾；名稱是原始文字），首頁寫成「特效：阿凱 訂閱了」「打卡卡片：小魚 7/10」「音效：喝水（橘子兌換）」。Host 自己看得到的問題會標在項目上（Overlay 不回報）：排隊中的項目帶 `onAir`（它的 Overlay 現在有沒有在 OBS 裡連上，Overlay 連上或斷線時重新發布；輪到時沒連上就跳過），首頁標「Overlay 沒開，不會播」；正在播的項目帶 `problem`，音效庫有登記但檔案已不在音效資料夾時為 `sound_missing`（特效照播，只是沒有聲音；兌換音效不送出，但照常佔用它的時間），首頁標「音效：檔案不見了」。`POST /api/admin/queue/skip`（跳過）只結束正在播的，並發布 `presentation.skipped`（帶它的 `stage`），該種 Overlay 收到後立刻拿掉畫面、停掉聲音，排隊中的隔一個間隔後照常接著播；`POST /api/admin/queue/clear` 取消排隊中的，正在播的播完；`POST /api/admin/queue/stop`（緊急停止）連正在播的一起停，並發布 `presentation.stopped`，各 Overlay 收到後立刻拿掉畫面、停掉聲音。三者都不影響兌換與打卡紀錄。佇列在每個項目結束時告訴它的模組結果（已播、音效檔不見、Overlay 沒開、模組關閉、跳過、清空、緊急停止）；特效模組在記憶體裡保留這次啟動最近 50 則特效與各自的結果（模擬的也算，標「模擬」），`GET /api/admin/alerts/recent` 依新到舊回傳，特效頁的「最近事件」每 2 秒讀一次；`POST /api/admin/alerts/recent/{id}/replay`（重播）用目前的樣板與音效把那一則再排進佇列，不再發布 `alert.received`、不寫活動紀錄，結果更新在原來那一筆。兌換音效（sound 模組）與集點卡（checkin 模組）也一樣，在記憶體裡保留這次啟動最近 50 筆與各自的結果：排進佇列時記下（還沒有結果），結束時記下上面那幾種結果；模擬的集點卡也算，標成模擬。兩者共用 `core` 的 `presentation::Recent`；每筆帶著它在模組裡對應的紀錄（兌換音效是 Twitch 的兌換 ID，集點卡是打卡紀錄的 ID；模擬的沒有），首頁的事件流依它對回結果。
  - 樣板：只有內建樣板 `default`、`banner`、`text`（原始檔在 `apps/web/src/overlay/alerts/templates/<id>/`，只用 HTML／CSS，同時內嵌進執行檔）；沒有資料夾樣板。選擇記在 `settings.json` 的 `alert_template`（預設 `default`，無法辨識的值當成 `default`），管理 UI 用 `GET`／`PUT /api/admin/templates/alerts`（`{ "selected": "<id>" }`；其餘值回 400 `template.unknown`，存檔失敗回 500 `settings.save_failed`）讀取與切換，存檔後發布 `template.changed`（`overlay` 為 `alerts`），Overlay 立即換掉。`GET /templates/alerts/{*path}` 提供目前選擇的樣板，`GET /templates/built-in/alerts/{id}/{*path}` 提供任一款給預覽（未知的 ID 回 404）；類型白名單、路徑檢查與標頭都和聊天室樣板相同。綁定方式：`data-alert-stage` 元素（目前播放的特效放在裡面）與 `<template data-alert>`（一則特效的標記）；`data-field="title"` 填「誰做了什麼」，`data-field="message"` 填觀眾留言（沒有留言就移除）；最上層元素加上 `data-kind`（事件種類）與 CSS 變數 `--duration`，時間到就移除。文字一律以 `textContent` 寫入；Overlay 一樣以 `DOMParser` 解析並移除會執行的內容，相對路徑指向樣板目錄。
  - 音效：預設音效 `chime.wav` 由 `scripts/generate-chime.mjs` 只用 Node 內建模組合成（不下載外部音檔，記在 `assets/CREDITS.md`），打包進建置產物。每種事件可在特效完整頁各自選內建音效（預設）、音效庫的一個音效或靜音（見「音效庫」）；payload 的 `sound` 告訴 Overlay 要播哪一個（`built_in` 帶 `volume`、`silent`，或 `file` 帶 `url` 與 `volume`），每則特效開始時播放一次。OBS 瀏覽器來源允許自動播放；要在 OBS 混音器調整音量，勾選來源的「透過 OBS 控制音訊」。
- **集點卡 Overlay**（`/overlay/checkin/`）：每次打卡成功（重複的不算）就把卡片排進同一條呈現佇列，輪到時送出一則 `checkin.stamped`：觀眾名稱、頭像網址、目前的格數與總格數、累計次數、是否剛好集滿、顯示時間（5 秒，之後隔 1 秒才送下一則），不放進快照。頭像向 Twitch（Helix Get Users）查一次後存起來，只接受 `https://static-cdn.jtvnw.net/` 的網址，Overlay 也再檢查一次。名稱以 Vue 的文字插值顯示。打卡設定可為一般打卡與集滿各選音效庫的一個音效（預設靜音），payload 的 `sound`（`url`、`volume`；靜音為 `null`）隨卡片播放一次。只有一款內建樣式（純 CSS，沒有外部素材），集滿時有不同的效果。模擬器（`POST /api/admin/simulate/checkin`，`source=simulation`）只送到 Overlay，不寫入紀錄。
  - 推卡與隱藏：打卡完整頁可把某位觀眾目前的集點卡推到畫面（`POST /api/admin/checkin/viewers/{id}/card`），和打卡的卡片一樣排進呈現佇列，但不寫打卡紀錄，事件流也不列；「隱藏畫面卡片」（`POST /api/admin/checkin/card/hide`）發布 `checkin.hidden`（不放進快照），Overlay 立刻收掉畫面上的卡片與音效。
  - 卡片樣式：存在 `settings.json` 的 `checkin.card`（內建樣式 `simple`／`journal`、背景顏色與圖片、印章顏色與圖片、每排 1–20 格、標題與說明最多 100 字、進場動畫 `slide`／`fade`／`pop`／`none`、重複打卡是否顯示）；改打卡設定不會動到它。管理 API：`GET`／`PUT /api/admin/checkin/card-style`、`POST /api/admin/checkin/images`（PNG、JPG、GIF、WebP，最多 5 MB，副檔名與 magic bytes 一致，存成 `checkin/<UUID>.<格式>`）、`POST /api/admin/checkin/card-style/test`（試播，`source=simulation`，不寫紀錄）。`checkin.stamped` 多帶代換好的 `title`、`description` 與 `style`（顏色、圖片網址、每排格數、樣式與動畫）；變數由 Host 代換，觀眾名稱最後代換，名稱裡的變數不會被代換；Overlay 以文字插值顯示，圖片只用 `/checkin/` 開頭的網址；卡片樣式的顏色與圖片優先於疊加層主題的底色。`GET /checkin/{name}` 與 `/music/{id}` 相同：唯讀、不需要 token、只提供符合 `<UUID>.<格式>` 且存在的檔案、`nosniff`、`no-store`。存檔時刪掉樣式不再使用的圖片。重複打卡要顯示時，聊天室指令與兌換的重複打卡排一張卡片（不寫紀錄、事件流不列）；重送的兌換事件不會再顯示。
  - 即時預覽：卡片樣式頁右邊用兩個 `sandbox="allow-scripts"` 的 iframe 載入 `/overlay/checkin/?preview`，分別顯示「打卡成功 +1」與「今天已打過」；管理視窗依表單內容組出範例卡片，以 `postMessage` 送進去，表單一改就重送。預覽模式不連 WebSocket、只收同一 origin 的父視窗訊息、卡片不自動收起也不播音效。
  - 動畫包：zip 內只能有 `pack.json`（`{"name": …}`，1–100 字）、`index.html` 與 CSS、PNG、JPG、GIF、WebP、WOFF、WOFF2、TTF、OTF、MP3、OGG、WAV；`.js`／`.mjs` 拒絕並指出檔名，`index.html` 有 `<script`、`on*=` 屬性或 `javascript:` 時拒絕並指出行與欄，SVG 與其他類型拒絕，每個檔案的副檔名與 magic bytes（CSS 須為 UTF-8）必須一致；zip 最多 10 MB、200 個檔案、解開後最多 20 MB（邊讀邊算，不信 zip 自報的大小），路徑不得有 `..`、絕對路徑、`\` 或 `:`。管理 API（Host）：`GET`／`POST /api/admin/checkin/packs`、`DELETE /api/admin/checkin/packs/{id}`；通過檢查才先寫到暫存資料夾再改名成 `templates/checkin/<UUID>/`，失敗不留任何檔案。`GET /templates/checkin/{id}/{*path}` 沿用樣板規則（白名單類型、Overlay CSP、`nosniff`、`Access-Control-Allow-Origin: *`），所以 sandbox 預覽框也讀得到；備份沿用樣板目錄。卡片樣式的內建樣式可選 `pack:<id>`（存檔時檢查動畫包存在）。Overlay 以 `[data-card-stage]` 與 `<template data-card>` 標記套用動畫包（同特效樣板，載入時移除 script、事件屬性與非 http(s)/data 網址），欄位以 `data-field` 指定（`title`、`description`、`name`、`count`、`banner`、`avatar`、`slots`），觀眾文字只當文字放入；動畫包不見或格式不對時改用內建卡片。動畫包只在 OBS 的 Overlay 與 sandbox 預覽框顯示，絕不在管理視窗顯示。
- **聊天指令**（`core` 的 `commands`，Host 組合）：各功能的聊天指令存在同一張表，`settings.json` 的 `chat_commands`。每筆以指令的 key 對應名稱、「誰能用」、所屬模組，以及實況主取的稱呼（`label`，例如計數器的名稱）；沒存過的用宣告的預設名稱、所有人可用。固定的指令由模組宣告（key、所屬模組、預設名稱），Host 把所有宣告組合成一張表交給各模組；實況主新增的指令（例如計數器的，key `counter.<ID>`）直接存進表，刪除時一併移除。模組只透過這張表存取自己的指令，不讀別的模組的資料。
  - 撞名：存檔時名稱和表裡其他指令（存過的，以及宣告了但沒存過的）相同（英文不分大小寫）就拒絕，不存任何東西；管理 API 回 409 `commands.taken`，並帶 `module`（用到它的模組）與 `label`（它的稱呼，沒有稱呼時是 `!<名稱>`），管理 UI 寫成：已被計數器的「死亡次數」使用。
  - 誰能用：所有人、訂閱者、VIP、管理員，所有功能用同一個判斷（看 Twitch 聊天徽章顯示的身分）。每一級也包含上面的級別（訂閱者 < VIP < 管理員），實況主本人永遠可以用所有指令。
  - 固定的指令目前只有打卡（key `checkin`）。舊版存在 `checkin.command` 的指令，在啟動時搬進表一次，之後不再寫入原位置；更早存的項目沒有所屬模組時，載入時依宣告補上。
- **音效庫**（sound 模組）：管理 API `GET`／`POST /api/admin/sounds`、`PUT`／`DELETE /api/admin/sounds/{id}`。
  - 匯入：body 就是檔案本身，`?name=` 帶原始檔名；上限 10 MB（`DefaultBodyLimit`，超過回 413 `sound.too_large`）。副檔名與檔案開頭的 magic bytes 必須是同一種格式：MP3（`ID3` 或 MPEG frame sync）、WAV（`RIFF`…`WAVE`）或 OGG（`OggS`），否則回 400 `sound.unsupported_type`。
  - 檔案複製到資料夾的 `sounds/`，以 UUID v7 命名；原始檔名只當名稱（以文字顯示），不當路徑。名稱、音量（0–100%）與格式記在 `settings.json` 的 `sound.library`。刪除時先從 `settings.json` 移除，再刪檔案。
  - `GET /sounds/{id}` 唯讀、不需要 token（給 Overlay 與管理 UI 試聽），只提供音效庫裡有的 ID，檔案路徑只由 ID 與格式組成。`Content-Type` 依格式固定（`audio/mpeg`／`audio/wav`／`audio/ogg`），另加 `X-Content-Type-Options: nosniff` 與 `Cache-Control: no-store`。
  - 選擇各處播放的音效：`PUT /api/admin/sounds/slots/{slot}`（`{ "sound": "<id>" | "silent" | null }`；`slot` 為 `alert.<種類>`、`checkin`、`checkin.full` 或 `reward.<Twitch 獎勵 ID>`），記在 `settings.json` 的 `sound.slots`；`null` 回到預設（特效是內建音效，其他是靜音）。alerts 與 checkin 模組在發布事件時才向 sound 模組的公開 service 查詢，所以模擬器觸發的特效與打卡也照設定播放。sound 模組停用時一律用預設。
  - 刪除音效時，用到它的 slot 一併回到預設；`GET /api/admin/sounds` 的每個音效帶 `used_by`（用到它的 slot），管理 UI 刪除前先列出。
  - 總音量：`PUT /api/admin/sounds/volume`（`{ "volume": 0–100 }`，記在 `settings.json` 的 `sound.volume`，預設 100%；`GET /api/admin/sounds` 帶 `volume`）。sound 模組查詢 slot 時把它乘上音效自己的音量，所以兌換、特效（含內建音效）與打卡音效都套用，「在 OBS 播放」也一樣；sound 模組停用時仍套用。在背景音樂播放器調整。
- **兌換音效 Overlay**（`/overlay/sound/`）：音效完整頁可把頻道點數獎勵（清單同打卡設定，Helix Get Custom Reward）對應到音效庫的音效。新的兌換（`RedemptionChange::Added`）由 Host 在交給 redemption 模組的同一處，也交給 sound 模組（記憶體，不經 outbox：晚播不如不播，重啟前的兌換不補播），sound 模組把它排進共用的呈現佇列（與特效、集點卡同一條），佔用的時間是音效的長度；輪到時發布 `sound.play`（`url`、`volume`），不放進快照。音效長度存在音效庫的項目（`duration_ms`）：管理視窗匯入後用 `<audio>` 讀取長度，以 `PUT /api/admin/sounds/{id}/duration` 存起來（1 毫秒到 6 小時）；管理視窗開啟時也會補量還沒有長度的音效。還沒量到長度時以 5 秒計。Overlay 依收到的順序一次播一個，播完（或播放失敗）才播下一個；等待超過 100 個時丟掉最舊的。「在 OBS 播放」（`POST /api/admin/sounds/{id}/play`）不經過佇列，立刻送到 Overlay（`source=simulation`），不寫入兌換紀錄。送出後按鈕變成「停止播放」，直到音效長度過去（還沒量到長度時，直到按下）；按下時 `POST /api/admin/sounds/stop` 發布 `sound.stopped`（不放進快照），Overlay 停掉正在播的測試音效、丟掉等待中的測試音效，兌換音效照常播放。OBS 加這個網址為瀏覽器來源，勾選「透過 OBS 控制音訊」。
- **計數器**（counter 模組，`/overlay/counter/`）：全域計數存在 `counter_counters`（名稱、目前數字、是否每場歸零、上次歸零的場次 ID），聊天指令與誰能用存在共用的聊天指令表（key `counter.<ID>`，`label` 是計數器名稱）。管理 API：`GET`／`POST /api/admin/counters`、`PUT`／`DELETE /api/admin/counters/{id}`、`POST /api/admin/counters/{id}/add`（`delta` 只能是 1 或 -1）、`POST /api/admin/counters/{id}/reset`。名稱 1–100 字；指令只能是一個詞（最多 25 字，可省略，`!` 會去掉）；最多 100 個。數字不會小於 0。
  - 聊天：訊息的第一個詞是某個計數器的 `!<指令>`（英文不分大小寫），而且傳送者能用，就加一。模擬事件（`source=simulation`）不算；模組停用時指令直接丟棄，加一、歸零按鈕與每場歸零照常。
  - 每場歸零：每次收到直播中的 `stream.status`（開播、每分鐘的檢查、重新啟動後都會再送），向 stream 模組的公開 service 取得目前場次 ID（`session_after_pending()`），每場歸零的計數器如果還沒為這一場歸零過，就歸零並記下這一場；所以每場只歸零一次，重新啟動或開播後才啟動工具（由每分鐘的檢查補上場次）都一樣。新增計數器，或把它改成每場歸零時，先記下目前的場次（直播中就是這一場），不會馬上歸零；還沒有任何場次時記成沒有，第一場開播時歸零。
  - 每次變動都發布 `counter.values`（所有計數器的 ID、名稱與數字，依新增順序），快照只保留最新一則。Overlay 列出每個計數器的名稱與數字（名稱以文字顯示）；計數器頁、首頁的計數器磚（顯示第一個計數器）與抽屜（可以加一）也依它更新數字。
  - 模擬事件視窗的「顯示測試值」（`POST /api/admin/simulate/overlay-test`，需要 token，計數器關閉時回 409）發布 `counter.test`（`source=simulation`，數字由 Host 固定為 99，不放進快照）：Overlay 另加一列樣式不同的「測試　99」，3 秒後消失，期間再按就重算 3 秒；真的 `counter.values` 照常更新。測試值不經過計數器本身，數字、存檔與聊天室指令都不受影響，管理視窗也不顯示它。
  - 每位觀眾各自的計數（例如「猜中死亡次數」）：`counter_viewer_counters`（名稱）與 `counter_viewer_counts`（計數、Twitch 使用者 ID、數字）。觀眾名稱由 viewers 模組的公開查詢提供（`viewers::search` 依登入名稱或顯示名稱找，英文不分大小寫，最多 20 位、最近出現的在前；`viewers::names` 依 ID 查），不跨模組 join 資料表。管理 API：`GET`／`POST /api/admin/counters/viewers`（每個計數帶總和與前三名）、`PUT`／`DELETE /api/admin/counters/viewers/{id}`（改名；刪除時連同每位觀眾的數字）、`GET /api/admin/counters/viewers/{id}/counts`（數字大於 0 的每個人，多的在前）、`GET /api/admin/counters/viewers/{id}/search?q=`（搜尋觀眾，附上他在這個計數的數字）、`POST /api/admin/counters/viewers/{id}/add`（`twitch_user_id` 與 `delta`，`delta` 只能是 1 或 -1；觀眾目錄裡沒有的觀眾回 404 `counter.viewer_not_found`）。數字不會小於 0；名稱以文字顯示。目前只能在計數器頁按按鈕加減，沒有聊天指令與 Overlay。
  - 訂閱馬拉松計時器（`/overlay/marathon/`，`?kind=marathon`）：存在 `counter_marathon`（一筆：剩餘時間、它是哪個時間點的值、是否手動暫停、計時用的場次 ID、每個訂閱加幾分鐘、每 100 小奇點加幾分鐘、最長幾小時）。管理 API：`GET /api/admin/counters/marathon`、`PUT /api/admin/counters/marathon/options`（分鐘 0–1440、最長 1–168 小時）、`POST /api/admin/counters/marathon/pause`（`paused`）、`POST /api/admin/counters/marathon/add`（`minutes` -1440 到 1440，不能是 0）。預設每個訂閱與每 100 小奇點都是 0 分鐘、最長 24 小時，設定前不會自己累積時間。
    - 加時：Host 在交給 activity 模組的同一處，也把特效事件交給計數器。訂閱與續訂各算一次；送禮訂閱只看送禮那一則的份數（`Gift.count`），Twitch 為每份另外送的 `Subscribe { gifted: true }` 不算，所以不會重複計算；小奇點依比例（50 小奇點算一半）。追隨、揪團不加時。模擬事件不加時；同一則（依 ID）重送只算一次；模組停用時直接丟棄。剩餘時間介於 0 和上限之間。
    - 時鐘：Host 擁有時鐘（Overlay 不回報）。剩餘時間記成「某個時間點的值」，只有在直播中、沒有手動暫停、還有時間時才往下扣；手動暫停不會因開播而解除。每次收到直播中的 `stream.status`（約每分鐘一次）就扣到現在並存檔；存下的時間點，也就是最後一次確認直播中的時候。
    - 重新啟動：記憶體裡先當作不知道是否直播中。第一個直播中的狀態如果還是同一場（場次 ID 相同），表示工具關著期間一直在直播，從存下的時間點扣到現在；如果是新的一場，就從它的開播時間扣起，前一場只扣到最後一次確認直播中的時間。第一個狀態是下播時，不扣關著的期間。工具開著時看到同一場先下播又回到直播中，中間的時間不扣。
    - 每次變動與每個直播中的狀態都發布 `counter.marathon`（剩餘時間、是否在倒數、是否暫停、是否直播中與三個設定），快照只保留最新一則；`occurredAt` 是剩餘時間的時間點，Overlay 與計數器頁在倒數中時自己往下算。倒數到 0 不會自動下播或切場景。
    - 同一條測試路由也能發布 `marathon.test`（`source=simulation`，剩餘時間由 Host 固定為 30 分鐘，不放進快照）：Overlay 以不同樣式顯示這個不倒數的時間 3 秒，再按就重算，之後回到真的倒數；計時器本身不變。
- **背景音樂庫**（bgm 模組）：和音效庫分開。管理 API `GET`／`POST /api/admin/music`、`PUT`／`DELETE /api/admin/music/{id}`（`PUT` 改曲名）。
  - 匯入與檔案比照音效庫：上限 50 MB（超過回 413 `music.too_large`），副檔名與 magic bytes 不一致回 400 `music.unsupported_type`（兩個模組共用 `core` 的 `audio` 檢查）；檔案複製到資料夾的 `music/`，以 UUID v7 命名。曲名、格式、長度與歌單記在 `settings.json` 的 `bgm`。刪除時先從 `settings.json`（含所有歌單）移除，再刪檔案。
  - `GET /music/{id}` 與 `/sounds/{id}` 相同：唯讀、不需要 token、只提供音樂庫裡有的 ID、`Content-Type` 依格式固定、`nosniff`、`no-store`，不送 `Access-Control-Allow-Origin`。另外支援單一的 byte range（回 206；格式不對或超出檔案回 416），Overlay 才能從歌曲中間開始播。
  - 長度：管理視窗匯入後用 `<audio>` 讀取，以 `PUT /api/admin/music/{id}/duration` 存起來（1 毫秒到 24 小時）；開啟背景音樂頁時也會補量還沒有長度的歌。`GET /api/admin/music` 的每首歌帶 `missing`：檔案不在 `music/`（在程式外被刪掉）時為 `true`，頁面標「檔案不見了」。
  - 歌單：`POST /api/admin/music/playlists`、`PUT`／`DELETE /api/admin/music/playlists/{id}`（改名、刪除；刪除歌單不刪歌）。一首歌可以在多個歌單；`PUT /api/admin/music/{id}/playlists`（`{ "playlists": [<歌單 ID>] }`）設定它在哪些歌單，新加入的排在該歌單最後，有不認得的 ID 時回 404 `music.not_found` 且不變動。
  - 播放器：Host 擁有時鐘，Overlay 不回報。播放狀態只放記憶體，重新啟動後是停止。
    - 控制：`POST /api/admin/music/player`，`action` 為 `play`（帶 `playlist`，沒帶就用上次選的；暫停中的同一個歌單接著播，其他情況從頭開始）、`pause`、`next`、`previous`（播超過 3 秒時回到這首開頭，否則上一首）、`stop`（保留選的歌單）或 `seek`（`position_ms`，超過長度就換下一首）。沒有選過歌單回 400 `music.invalid`，不認得的歌單回 404 `music.not_found`。
    - 選項：`PUT /api/admin/music/player/options`（`volume` 0–100%、`shuffle`），記在 `settings.json` 的 `bgm.volume`（預設 50%）與 `bgm.shuffle`。
    - 順序：每一輪開始時決定順序（隨機時打亂）；播完最後一首就再開一輪，隨機時重新打亂，歌單的修改也從這時生效。切換隨機時，正在播的這首不變，其餘的歌重新排序。檔案不見、還沒量到長度或已從音樂庫刪除的歌都跳過；整個歌單都不能播時，`play` 回 409 `music.nothing_to_play`，播放中則停止。
    - 時鐘：位置依量到的長度前進，播滿就換下一首。只在播放中、而且背景音樂 Overlay（`/overlay/bgm/`，`?kind=bgm`）有連線時前進；沒有連線時位置停住並標 `onAir: false`，管理 UI 顯示「Overlay 沒有連線」，不假裝在播。OBS 重新載入來源時，斷線期間時鐘停住，連回後從原位置接著播。
    - 事件 `bgm.player`，快照只保留最新一則：`state`（`stopped`／`playing`／`paused`）、`playlist`、`track`（`id`、`title`、`url`、`durationMs`）、`positionMs`（發布當下的位置）、`onAir`、`shuffle`、`volume`。Overlay 以 `positionMs` 加上 `occurredAt` 之後經過的時間算出目前位置（shortcut：假設 Overlay 和 Host 在同一台電腦、時鐘相同），和實際播放差超過 1 秒才跳轉，斷線時暫停。背景音樂頁、首頁的背景音樂磚與抽屜都依它顯示。OBS 加這個網址為瀏覽器來源，勾選「透過 OBS 控制音訊」。
    - 模擬事件視窗的「播 5 秒」（`POST /api/admin/music/player/test`，需要 token）發布 `bgm.test`（`source=simulation`，不放進快照）：`track` 取自最後選的歌單，依歌單順序找第一首能播的曲目；另有 `volume` 與 `durationMs`（5 秒）。Overlay 用另一個音訊元件從頭播，不經過播放器，時間到、再來一則測試，或收到 `state=playing` 的 `bgm.player` 時就停，所以不會和真的播放同時出聲。播放器的狀態、曲目與位置都不變。功能關閉、沒選過歌單、歌單沒有能播的曲目或背景音樂正在播時，Host 回 409（`music.off`、`music.nothing_to_play`、`music.playing`），視窗也先把按鈕停用並說明原因。
  - 場景對應：`PUT /api/admin/music/scene-rules` 整份取代（依序比對；新的對應 `id` 留空，由 Host 給 UUID v7），記在 `settings.json` 的 `bgm.scene_rules`；`GET /api/admin/music` 帶 `scene_rules`。每個對應有名稱、場景（可多個，去頭尾空白，重複的不分大小寫只留一個）、歌單、播放方式（`shuffle`）、音量、進場自動播（`autoplay`）、離開時停止（`stop_on_leave`）與啟用（`enabled`）。啟用的對應一定要有歌單，否則回 400 `music.invalid`；不認得的歌單回 404 `music.not_found`。存檔不會馬上套用到目前的場景，下次切場景才生效。
    - 切場景：Host 跟著 OBS 的場景變化（`StreamingState`），場景名稱比對不分大小寫。同一個場景不動作。切到沒有對應的場景（停用的、沒有歌單的對應都不算）一律停止。切到開啟「進場自動播」的對應，就播這個對應的歌單，播放方式與音量也照它設定：正在播的歌也在新歌單裡就繼續播、不中斷，否則從頭開始。切到關閉「進場自動播」的對應時，看原場景的對應：原場景的「離開時停止」開著就停止，其他情況繼續播。
    - OBS 連上時先讀目前的場景並套用（視同從沒有場景切過去）；OBS 斷線視同切到沒有對應的場景，所以會停止。
    - 一個對應都沒有時，切場景與 OBS 斷線都不影響播放器，讓沒有用場景對應的人照樣手動播放。
    - 刪除：`GET /api/admin/music` 的歌單與歌都帶 `used_by`（播它的、或歌單裡有它的場景對應名稱），刪除前的確認框列出。刪除歌單時，用到它的對應改成沒有歌單並停用，頁面顯示「歌單不見了」，不會換成別的歌單；刪除歌曲只列出，不停用對應。
  - bgm 模組停用時隱藏背景音樂頁與首頁磚；`play` 回 409 `music.off`，正在播的播完這首就停止，切場景也不影響播放器。
- **畫面工具**（overlays 模組，模組 ID 與頁面路徑都是 `screen-tools`，首頁磚沿用還沒做時的 ID）：
  - 休息中／準備中卡片（`/overlay/break/`，`?kind=break`）：存在 `overlays_break_card`（一筆：文字、倒數結束後的文字、倒數方式 `minutes`／`until`、分鐘、到幾點 `HH:MM`、是否顯示、倒數結束的時間）。管理 API：`GET`／`PUT /api/admin/screen-tools/break`（文字最多 100 字、分鐘 1–1440）、`POST /api/admin/screen-tools/break/show`（`minutes` 或 `until` 擇一；`until` 是 24 小時內的 UTC 時間，「到幾點」由管理視窗換算成下一次到那個時間的時刻）、`POST /api/admin/screen-tools/break/hide`。
  - 顯示時存下倒數結束的時間，重新啟動工具或重新載入 Overlay 都顯示同一個時間；每次變動與啟動時發布 `screen.break`（整張卡片），快照只保留最新一則。Overlay 自己往下算，到 0 換成結束文字，卡片留著直到手動隱藏；倒數結束不會自動切場景。
  - 警語（`/overlay/warnings/`，`?kind=warnings`）：出問題時在直播畫面上用小字提醒，觀眾也看得到。設定存在 `overlays_warnings`（一筆：三個開關、麥克風靜音幾秒才提醒、要看的 OBS 音訊輸入名稱）；預設開「聊天室 Overlay 沒連上」與「麥克風靜音超過 30 秒」，Twitch 中斷預設關。管理 API：`GET`／`PUT /api/admin/screen-tools/warnings`（秒數 1–3600）。
    - Host 每 0.5 秒把看到的狀態交給模組：有沒有開著的聊天室 Overlay（`?kind=chat` 的連線數）、Twitch EventSub 是否連上、OBS 的音訊輸入（OBS 沒連上時為未知）。模組只在狀態或設定改變時發布 `screen.warnings`（設定加上狀態），快照只保留最新一則。
    - 麥克風：從 OBS 的輸入清單選（清單本來就只列有靜音狀態的輸入，也就是有音訊的輸入）；沒選就不檢查。模組記下第一次看到靜音的時間，Overlay 在滿設定的秒數後顯示，取消靜音就消失；換一個輸入重新計時。OBS 連著但清單裡沒有選的輸入時標記 `micMissing`，畫面工具頁提示重新選擇，此時不檢查靜音。
  - 疊加層外觀：內建 Overlay 的主題（`dark` 深色、`cream` 奶油、`contrast` 高對比）與文字大小（50–200%，每 10%），和管理介面的主題分開。存在 `overlays_appearance`（一筆）；管理 API `GET`／`PUT /api/admin/screen-tools/appearance`；每次變動與啟動時發布 `screen.appearance`，快照只保留最新一則，已開的 Overlay 立刻換，不用重新整理。
    - 套用方式：Overlay 頁收到後在頁面根節點設 CSS 變數（`--ov-fg` 文字、`--ov-bg` 底色、`--ov-accent` 強調色、`--ov-shadow` 文字陰影、`--ov-scale` 文字倍率）；深色、100% 不設任何變數，就是內建 Overlay 原本的樣子。內建 Overlay（休息卡片、警語、計數器、馬拉松計時器、集點卡）與聊天室、特效的內建樣板讀這些變數，原本的值當備援；使用者自訂樣板不讀，所以不受影響。Host 的測試檢查每個內建樣板的 CSS 都有讀主題與文字倍率。
    - 之後新增的內建 Overlay（例如排行榜）也要呼叫 `apps/web/src/overlay/appearance.ts` 的 `followAppearance`，並讀同一組變數。
  - 媒體庫（兌換時播的影片與 GIF）：管理 API `GET`／`POST /api/admin/screen-tools/media`、`PUT`／`DELETE /api/admin/screen-tools/media/{id}`（`PUT` 改名稱與長度）。清單存在 `overlays_media`（UUID v7、名稱、格式、長度），檔案在資料夾的 `media/<id>.<格式>`，隨備份一起備份與還原。
    - 匯入：body 就是檔案本身，`?name=` 帶原檔名（只用來看副檔名與當作預設名稱，不當成路徑）；只收 GIF、WebM、MP4，上限 50 MB（超過回 413 `media.too_large`），副檔名與 magic bytes（GIF 的 `GIF87a`／`GIF89a`、WebM 的 EBML 開頭、MP4 的 `ftyp`）不一致回 400 `media.unsupported_type`。
    - 長度：GIF 預設 5 秒，可以改（1 毫秒到 10 分鐘）；影片由管理視窗匯入後用 `<video>` 讀取再存起來，開啟畫面工具頁時也會補讀還沒有長度的影片。
    - `GET /media/{id}` 與 `/music/{id}` 相同：唯讀、不需要 token、只提供媒體庫裡有的 ID、`Content-Type` 依格式固定、`nosniff`、`no-store`；目前不支援 byte range（短片從頭播放用不到）。
  - 兌換時播影片或 GIF：每個頻道點數獎勵（清單同打卡設定）最多對應一個媒體庫項目，存在 `overlays_media_rules`（獎勵 ID、項目、位置 `top_left`／`top_right`／`bottom_left`／`bottom_right`／`center`／`full`、音量；音量 `null` 表示跟著音效總音量）。管理 API：`GET /api/admin/screen-tools/media-rules`、`PUT`／`DELETE /api/admin/screen-tools/media-rules/{reward_id}`、`POST /api/admin/screen-tools/media-rules/{reward_id}/test`（試播）。刪除媒體項目時，用到它的對應一併刪除。
    - 播放：Host 在交給音效模組的同一處，也把兌換交給這個模組。新的兌換（狀態更新不算、模組停用時不算）依對應排進呈現佇列（`Stage::Alerts`，佇列項目 `media`），在特效 Overlay 播放，不用另外加來源；輪到時發布 `screen.media`（網址、格式、位置、音量、長度，不放進快照），特效 Overlay 依位置顯示 GIF 或影片，影片照播聲音；被跳過或緊急停止時立刻拿掉。影片還沒讀到長度時不排隊。
    - 檔案不見時，輪到那一項就回「被模組拒絕」（不佔時間、不卡住後面的項目），模組記下原因，事件流寫明「檔案不見了」。對應與媒體庫留在記憶體，兌換一到就能同步排隊；重新啟動前的兌換不會補播。
    - 試播直接發布 `source=simulation` 的 `screen.media`，不經過呈現佇列，也不留任何紀錄。
- **Host 標頭**：只接受 `127.0.0.1:<port>` 與 `localhost:<port>`，擋 DNS rebinding。
- **Origin**：所有 WebSocket 連線（含 Overlay）與會改變狀態的請求都檢查 Origin，只接受 `http://127.0.0.1:<port>` 與 `http://localhost:<port>`（目前的 port）；dev 模式另外接受 Vite dev server 的 origin（由 `apps/desktop` 以執行期設定傳入）。沒有 Origin 或 `Origin: null` 的 WebSocket 連線一律拒絕。
- **CORS**：`/api`、`/ws`、`/admin` 不開。只有建置產物的 `assets/` 目錄（管理 UI 與 Overlay 共用 chunk）與樣板路徑（`/templates`，含內建樣板）送 `Access-Control-Allow-Origin: *`（不帶憑證，內容都是公開程式碼與樣板），因為預覽 iframe 的 origin 是 `null`，Vite 產出的 module script、CSS 與字型都要經 CORS 才能載入。
- **Overlay**：路由唯讀，只能讀快照與訂閱公開事件，沒有任何會改變狀態的端點；OBS 網址不含 token。
- **OBS Dock**：目前不做；之後與手機遠端控制一起設計配對方式。

### Twitch 流量限制
數字以 Twitch 官方文件為準；Twitch 調整時更新這裡。
- **Helix API**：Twitch 依 Client ID 與使用者分配點數。所有 Helix 請求共用同一套處理。401 只更新權杖並重試一次。點數歸零或收到帶有 `Ratelimit-Reset` 的 429 時，同一登入帳號暫停新請求，直到重設時間（最多等 1 分鐘）。沒有重設時間的 429 由呼叫端依各自的方式退避。Helix 與 OAuth 請求在 15 秒內未收到回應時視為連線失敗。
- **EventSub WebSocket**：每個使用者 token（Client ID＋使用者）最多 3 條有訂閱的連線、每條最多 300 個訂閱、`max_total_cost` 為 10。本工具每個帳號只開 1 條連線，換 Client ID 或登出時先關掉舊連線；收到 `session_reconnect` 時改連 Twitch 給的 reconnect URL（不算新連線，訂閱沿用，不重新訂閱）；連線失敗以指數退避重連（1 秒起，最多 60 秒）。
- **直播狀態與場次**（stream 模組）：訂閱 `stream.online`、`stream.offline`（不需要權限，各佔 `max_total_cost` 的 1），只有 `type` 為 `live` 的開播才算；登入期間另外每 60 秒以 Helix Get Streams 檢查一次（登入時先查一次），接上程式啟動前或 EventSub 斷線時已經開始的直播，並更新觀眾數。每次 Twitch 直播是一場，存在 `stream_sessions`（以 Twitch 的直播 ID 為唯一鍵，同一場收到兩次只算一場；開播時間用 Twitch 的時間）；下播後這一場仍是目前的場次，直到下次開播；開新的一場時，其他還沒結束的場次（漏接下播，例如程式在直播中關掉、下一場才重開）以新一場的開播時間結束。登出 Twitch 時狀態改為 `unknown`，場次不變（登出無法證明已下播）。Get Streams 會落後事件約一分鐘，所以：檢查結果仍列出已由 `stream.offline` 結束的那場時不重新開場；比目前場次更早開播的直播（落後的檢查或事件）一律忽略，舊的一場也不會結束較晚開播的場次；檢查說沒有直播時，只有開播超過 3 分鐘的場次才當作下播（補上漏掉的 `stream.offline`）。狀態以事件 `stream.status` 發布（`state` 為 `unknown`／`offline`／`live`，直播中另帶 `startedAt` 與 `viewers`），快照只保留最新一則；管理 UI 頁首依它顯示「直播中」、時長與觀眾數，或「已下播」。每場另存最高觀眾數（直播中每次檢查取較大值），重啟後保留。
- **本場統計**（Host 組合）：`GET /api/admin/stream/summary` 回傳最新一場（直播中的，或下播後的上一場，直到下次開播）的開始與結束時間、最高觀眾，以及這一場的追隨、訂閱（新訂閱與續訂，送出的每份訂閱都算）、小奇點、揪團與兌換數；還沒有任何一場時回傳 `null`。各數字由各模組查自己的表（活動紀錄依場次 ID、兌換紀錄依這一場的時間範圍），不跨模組 join；另帶 `endedSinceLaunch`（這一場在這次啟動後才結束）。首頁頂端（「編輯首頁」左邊）的直播階段切換依 `stream.status` 與它自動切換：直播中顯示「直播中」；這次啟動後才下播的顯示「下播後」；其他情況（重啟後、登出、沒有開 Twitch 直播）顯示「開播前」；實況主可以手動切到別的階段看，直到下次狀態改變。每個階段有自己的首頁排列，「編輯首頁」只改目前選的階段（編輯時也能切換階段），存在 `settings.json`：直播中是 `home_tiles`（以前唯一的那份），開播前、下播後是 `home_tiles_before`、`home_tiles_after`；`GET`／`PUT /api/admin/home-tiles?stage=before|live|after`（預設 `live`）各讀寫一份，檢查相同，`null` 是還沒排過、用該階段的預設。預設：開播前是「開播前檢查」加場景、背景音樂、音效、特效、聊天室；直播中是「待處理」（有項目時才顯示）、「本場直播」（直播中的數字；平常是小磚，顯示時間與兌換、訂閱數）、事件流、「呈現佇列」（平常是小磚，顯示排隊數與正在播什麼）加全部功能磚；下播後是「本場摘要」（可匯出）加計數器、兌換紀錄、打卡、聊天室。以前存的排列算直播中的，Host 第一次讀取時把待處理、本場直播、呈現佇列放在最前面（`home_tiles_version`）。直播中每 30 秒重新讀取一次數字。「開播前」列出開播前檢查與「N / M 好了」：Twitch 已登入、EventSub 已連線（`GET /api/admin/twitch` 的 `eventsub_connected`：目前的 EventSub 連線完成訂閱後為 `true`，連線結束或登出時為 `false`；換登入時舊連線的回報不算）、OBS 已連線，以及每個開啟中功能的 Overlay 在 OBS 裡有連上（計數器與馬拉松計時器的 Overlay 只是把數字顯示在畫面上，可有可無，不列入；`GET /api/admin/overlays`，停在「開播前」時每 3 秒讀一次）；沒好的 Twitch、EventSub、OBS 項目有「修正」，帶到設定的連線區；沒放進 OBS 的 Overlay 項目直接有「複製 OBS 網址」，複製後提示在 OBS 新增「瀏覽器」來源貼上。沒放進 OBS 的 Overlay 算提醒（黃色），其他沒好的項目算問題（紅色）。之後的功能各自加上自己的檢查項目。「下播後」的摘要有「匯出本場紀錄」：`GET /api/admin/stream/record` 回傳最新一場的追隨、訂閱與續訂（送禮送出的每份訂閱各一列，送禮本身不另列）、小奇點、揪團與兌換，一筆一列，和本場統計算的一樣（模擬事件本來就不存，所以不含）；管理 UI 存成 CSV（時間、事件種類、觀眾、數量或內容），UTF-8 加 BOM、CRLF 換行、每格加引號，開頭是 `=`、`+`、`-`、`@`、Tab、CR 或換行字元的格子前面加 `'`，試算表不會當成公式。存檔和備份匯出一樣，先問存在哪裡（`showSaveFilePicker`），沒有這個 API 時改成下載。
- **事件流**（Host 組合）：`GET /api/admin/feed` 依新到舊列出本場（最新的場次，從開播時間起；還沒有任何場次時從這次啟動起）最近 50 件特效、兌換與打卡：誰、做了什麼、觸發了什麼（特效、兌換音效與它的名稱、集點卡、兌換的影片或 GIF 與它的名稱；同一個獎勵有音效也有影片時列音效）與結果（排隊中、已播、已播但音效檔案不見了、Overlay 沒開沒有播、被模組拒絕（Host 知道原因時一併寫出，目前只有「檔案不見了」）、已跳過、已清空排隊、緊急停止）。activity、redemption、checkin 模組各自依時間範圍回答自己的資料表（`activity_between`、`redeemed_between`、`checked_in_between`），送禮時另外存的每份訂閱不另列；結果取自 alerts、sound、checkin、overlays 模組記憶體裡的最近紀錄，所以只有這次啟動的才有，重新啟動後舊事件只顯示事件本身。模擬事件不進資料表，只從記憶體裡的特效與集點卡取得，標「測試」。首頁「事件流」磚（不屬於任何模組，一定顯示）顯示件數與最新一件，點一下在首頁展開成全部的清單；本場直播、事件流、呈現佇列的小磚點一下展開成大區塊（本場直播佔一整列、事件流約三分之二、佇列約三分之一），區塊上的「收合」變回小磚，焦點留在原處；展開只在這次開啟中有效，不存檔。收到可能改變它的即時事件時，最多每秒重新讀一次。
- **活動紀錄**（activity 模組）：來自 Twitch 的追隨、訂閱、續訂、送禮（含送出的每份訂閱）、小奇點與揪團各存一筆在 `activity_events`，帶事件到達當下的場次 ID（stream 模組依收到順序回答，開播後緊接的事件一定算新的一場；不跨模組 join）；以 EventSub 訊息 ID 為唯一鍵，同一則重送只存一筆；模擬的事件不存。紀錄保存期限存在 `settings.json` 的 `record_retention`（`{"days": N}`，N 為 1 到 3650，或 `"forever"`；沒設定時為 30 天，聊天紀錄共用），啟動時與每天刪掉超過期限的紀錄；讀不到 `settings.json` 或內容不合法時這次不刪，不會照預設值猜。管理 API：`GET`／`PUT /api/admin/records/retention`（`PUT` 在同一個資料庫交易裡刪掉新期限不再保留的紀錄，存好 `settings.json` 才提交，回傳刪了幾筆；存檔失敗時刪除一併取消；管理 UI 出錯後重新讀取實際生效的期限）、`GET /api/admin/records/expired?days=N`（改成 N 天會刪幾筆）；設定「資料與備份」改短期限前先顯示會刪幾筆並確認。紀錄在資料庫裡，會隨備份匯出與還原。其他模組的紀錄表（目前是聊天紀錄）以 `core` 的 `RecordTable` 交給 activity 模組，由各模組自己的 SQL 刪除與計數：每日清除、改期限時的同一個交易與「會刪幾筆」都把它們算進去，activity 模組不碰別人的表。
- **聊天紀錄**（chat 模組）：來自 Twitch 的聊天訊息（模擬的不存；聊天模組停用時不收）存在 `chat_messages`，帶訊息到達當下的場次 ID（同活動紀錄的問法），以 Twitch 訊息 ID 為唯一鍵；`payload` 欄存 `chat.message` 的 payload。寫入在背景依收到順序批次進行（每次最多 200 則）。管理員刪除訊息、清除某位觀眾或整個聊天室時（`channel.chat.message_delete`、`channel.chat.clear_user_messages`、`channel.chat.clear`），存下的訊息清空文字與片段、標記已刪除，不刪掉那一筆；清除觀眾或整個聊天室只回溯 24 小時內的訊息；同時發布 `chat.removed`（`messageId`、`userId`，兩者都沒有表示整個聊天室），並把那些訊息從快照拿掉，聊天室 Overlay 收到後移除，管理視窗的聊天室改顯示「這則訊息已被管理員刪除」。`GET /api/admin/chat/messages` 回傳最近 200 則（含已刪除的標記），管理視窗開啟時把它們放在即時訊息前面，所以重啟後仍看得到之前的聊天；OBS 的聊天室 Overlay 仍只拿快照，重啟後不會出現舊訊息。本場統計另帶這一場的聊天訊息數（`chatMessages`）。聊天訊息和觀眾更新各以最多 75 毫秒、200 筆為一批寫入，管理事件會先提交前面的聊天訊息。佇列滿時，管理事件會等候空位。從系統匣離開或由程式重啟時，會等候已接受的聊天寫入完成。缺少頻道歸屬的舊紀錄可依訊息 ID 單則刪除。
- **管理 UI 的聊天室顯示**：`chat.message` 的 payload 另帶 Twitch 原本的每個徽章（`twitchBadges`：`setId`、`id`、`info`，依 Twitch 的順序；`badges` 仍是身分），共享聊天時來自其他頻道的訊息帶 `sharedFrom`（那個頻道；本頻道自己的訊息與非共享聊天都沒有）。`GET /api/admin/chat/badges` 回傳 Twitch 的全域徽章與實況主頻道的徽章（Helix Get Global／Channel Chat Badges，不需要權限），每次啟動讀一次後留在記憶體，只留圖片在 Twitch CDN 的；登出或讀不到時回傳空的，聊天室就不顯示徽章圖。`GET /api/admin/viewers/avatars?ids=` 回傳觀眾模組存下的頭像（一次最多 200 位）；新觀眾的頭像在背景查詢，管理 UI 查不到時隔一段時間再問，最多三次。聊天室顯示設定（風格精簡／標準／寬鬆、頭像、徽章、時間、字級 80–150%）存在 `settings.json` 的 `chat_display`，`GET`／`PUT /api/admin/chat-display`，聊天紀錄頁與常駐聊天室共用。共享聊天時兩者上方顯示「共享聊天中」與對方頻道（從收到的訊息得知），可以切「只看本頻道」。OBS 的聊天室 Overlay 樣板不變。
- **第一次聊天**：EventSub 沒有「觀眾在這個頻道的第一則訊息」的標記，所以登入期間另開一條 Twitch IRC 連線（`wss://irc-ws.chat.twitch.tv:443`，以 `justinfan` 訪客身分匿名、唯讀，不需要登入或權限，只送 `CAP REQ :twitch.tv/tags twitch.tv/commands`、`NICK`、`JOIN #<實況主帳號>` 與回應 `PING`，帳號名稱先檢查格式），讀 `PRIVMSG` 的 `first-msg=1` 標籤，用它的 `id` 對上 EventSub 的訊息 ID（Twitch 的 Delete Chat Messages 文件寫明訊息 ID 就是 IRC 的 `id` 標籤，EventSub 的訊息 ID 也用在同一個 API；尚未用真的訊息確認，對不上的標記就不顯示）。連線與 EventSub 一起在登入後開始、登出時結束，斷線以同樣的指數退避重連；連線狀態以 `GET /api/admin/twitch` 的 `irc_connected` 表示，設定的 Twitch 區顯示目前用哪一種判定。IRC 沒連上時改用觀眾模組判定：本工具第一次看到這位觀眾的訊息算第一次聊天（不等於他在頻道的第一次）。標記可能比訊息早或晚到：早到的先記著（最多 200 個），訊息到時直接帶 `firstChat: true`；晚到的發布 `chat.first`（`messageId`），並更新快照裡那則訊息。存下的訊息記在 `chat_messages` 的 `first_chat` 欄；本場統計另帶這一場的第一次聊天數（`firstChats`），聊天紀錄頁多一個「第一次聊天」篩選，聊天室在訊息旁標「第一次聊天」。
- IRC 收到兩項能力的確認且成功加入頻道後，才啟用首聊判定。批次清除只影響接收事件的頻道與最近 24 小時；舊資料若沒有頻道 ID，批次清除會保留原文。管理頁重連時會同步最近的刪除與首聊標記。Host 執行期間，聊天寫入失敗會依原順序重試，每次等待最長 30 秒。 寫入工作每批最多 200 筆，另可排隊 2,048 筆，其中 64 筆保留給刪除事件。超出容量時，仍顯示即時聊天，但停止接受超額寫入。兩個管理聊天室畫面都會顯示本次啟動累計未儲存的操作數。
- **聊天紀錄頁**（Host 組合）：`GET /api/admin/chat/periods` 依新到舊列出最近 50 場直播（開始到結束）與兩場之間的聊天（第一場之前、上一場結束之後也算；沒有聊天的空檔不列），每段帶訊息數與第一則的時間，頁面標成「11/16 的直播」或「11/15（沒有直播）」。`GET /api/admin/chat/log?from=&to=` 回傳那段時間存下的訊息（最多最新的 5000 則，超過時標 `truncated`），以及活動紀錄與兌換紀錄（最多各 2000 筆；送禮送出的每份訂閱不另列，由送禮那一筆代表），各模組查自己的表，依時間交錯排列。「即時」則是管理視窗的即時訊息（先載入最近 200 則），加上開頁後收到的特效。篩選（全部、提到我、!指令、管理員和 VIP、訂閱者、醒目訊息和小奇點、兌換、訂閱揪團和追隨）、搜尋與「隱藏機器人」（內建的常見機器人名單）可以一起用，都在管理 UI 裡做。 管理視窗重連後會重讀選定的歷史區間，並把快照中的刪除事件套用到查詢結果。即時聊天的歷史紀錄讀取失敗時會顯示提示並自動重試。歷史頁也會補上重連快照中的首次聊天標記。非直播時段收到新訊息而場次尚未列出時，會在短暫延遲後重查場次。
- **觀眾名冊**（viewers 模組）：在聊天、特效、兌換出現過的 Twitch 使用者各一筆，存在 `viewers`（以 Twitch 使用者 ID 為主鍵，改名後更新同一筆）：登入名稱、顯示名稱、頭像、最近一次聊天徽章顯示的身分（實況主、管理員、VIP、訂閱者；只有聊天帶徽章，其他事件不改身分）、第一次與最後出現時間。沒有頭像的觀眾向 Twitch（Helix Get Users）每批最多查 100 人，暫時失敗最多再試兩次，只存 `https://static-cdn.jtvnw.net/` 的網址（與打卡共用 `core` 的檢查與查詢介面）。模擬的事件不建立也不更改觀眾；打卡自己的觀眾資料不搬過來。其他模組只透過公開查詢取得名稱（`search`、`names`、`avatars`），不讀這個資料表。
  - 每條連線訂閱 14 個事件：`channel.chat.message`；特效用的 `channel.follow`（v2）、`channel.subscribe`、`channel.subscription.message`、`channel.subscription.gift`、`channel.cheer`、`channel.raid`（條件為 `to_broadcaster_user_id`）；兌換紀錄用的 `channel.channel_points_custom_reward_redemption.add` 與 `.update`；直播狀態用的 `stream.online`、`stream.offline`；聊天紀錄用的 `channel.chat.message_delete`、`channel.chat.clear_user_messages`、`channel.chat.clear`（與 `channel.chat.message` 相同的條件與權限 `user:read:chat`）。揪團與直播狀態不需要實況主授權，各為 cost 1；其他由實況主授權，cost 為 0，合計 3，遠低於 10。
  - 更新前的登入缺少某個事件所需 scope 時，不訂閱那個事件；Twitch 對聊天以外的訂閱回錯誤狀態碼（含 429，或頻道沒有頻道點數）時只記錄警告，聊天與其他事件照常。聊天訂閱失敗，或任何訂閱遇到登入失效、連不上 Twitch，才算連線失敗並重連。
- **Device Code Flow**：依 Twitch 回傳的 `interval` 輪詢到 `expires_in` 為止，不自行加快；間隔最少 1 秒。
- **聊天發訊**：目前沒有發訊功能。之後要發訊（例如打卡回覆）時，必須在本機排隊並限速，採保守值 20 則／30 秒。Twitch 的上限是一般帳號 20 則／30 秒、實況主／mod／VIP 100 則／30 秒（實況主發的訊息也會算進 20 則的桶）；超過時 Twitch 會忽略該帳號的訊息 1 小時。

### OBS 連線
- 用 `obws` 連 OBS 內建的 obs-websocket 5，不開 TLS。位址存在 `settings.json` 的 `obs.host`／`obs.port`（預設 `127.0.0.1`／`4455`）；密碼存 Windows Credential Manager，取出後立即包成 `SecretString`，不寫入 `settings.json` 與 log；沒設密碼就不帶驗證連線（OBS 關閉驗證時這是正常狀態）。交握時只送 challenge 回應，不送密碼本身。
- 直播軟體介面：場景、物件、輸入與輸出的狀態定義在 `core` 的 `streaming`（`StreamingState` 與 `StreamingSoftware` trait），OBS 只是它的一種實作；之後的場景、模式等功能只透過這個介面，不直接用 `obws`。指令目前有切換場景、顯示或隱藏物件、鎖定物件，透過狀態所用的同一條連線送出；結果不從指令的回應更新，而是等 OBS 的事件反映到狀態。靜音等其他指令等功能需要時再加。
- 連上後先讀一次完整狀態（目前場景、每個場景的物件與是否顯示、是否鎖定、來源類型，物件依 OBS 來源清單由前到後排列；每個可以靜音的輸入是否靜音（圖片、顯示器擷取這類沒有聲音的輸入，OBS 回報不支援音訊，就不列出，也不中斷連線）、是否在串流與錄影），之後依 OBS 事件更新：切場景、物件顯示與鎖定、靜音、串流與錄影狀態直接更新；新增、刪除、改名或重排場景、物件、輸入時重新讀一次。群組裡的物件不列出。斷線時狀態清空（場景為 `null`，表示未知），重連後重新讀取。狀態以事件 `obs.state` 發布，`payload` 是整份狀態；管理 UI 頁首依它顯示「串流中」「錄影中」。
- 連不上或斷線時以指數退避重連（1 秒起，最多 60 秒，與 EventSub 相同）；連上後重新從 1 秒計算。每次連線嘗試（含交握與版本檢查）最多 10 秒。
- 狀態（事件 `obs.status`，`payload` 為 `state` 與 `reason`；`reason` 是固定代碼，`connected` 與剛啟動時為 `null`）：
  - `connected`：已連上。
  - `reconnecting`：還沒連上，或會自己恢復，例如 OBS 沒開（`obs.unreachable`）、OBS 關閉（`obs.disconnected`）、連上但交握失敗或沒有在時限內完成（`obs.handshake_failed`，例如對方不是 obs-websocket）。OBS 沒開是正常狀態，不算錯誤。
  - `error`：仍會重試，但要使用者處理，例如密碼錯誤（`obs.auth_failed`）、OBS 要求密碼但沒有設定（`obs.password_required`）、版本不支援（`obs.unsupported_version`，需要 OBS 30.2 以上、obs-websocket 5.5 以上的 5.x）。
- log 只記狀態變化，OBS 長時間沒開也不會一直寫 log。
- 管理 API `/api/admin/obs`：
  - `GET` 回傳位址、是否已設密碼（`password_set`，不回傳密碼本身）與目前狀態；讀不到 Credential Manager 時回 500 `obs.secret_store_failed`。
  - `PUT` 修改位址與密碼（`password` 省略表示不變，空字串表示刪除），修改後立即用新設定重連，並清掉舊連線的場景與輸出狀態；被取代的舊連線不再寫入狀態。host 只接受主機名稱與 IPv4、port 不得為 0，否則回 400 `obs.invalid_address`。
  - `PUT` 先存 `settings.json` 再寫密碼：密碼寫入失敗時新位址仍會生效，回 500 `obs.secret_store_failed`；存不了 `settings.json` 時回 500 `settings.save_failed`，位址、密碼與連線都維持原樣。
- 管理 API `PUT /api/admin/obs/scene`（切換場景，`scene`）與 `PUT /api/admin/obs/item`（`scene`、`item` 加上 `enabled` 和／或 `locked`，兩者都沒給回 400 `obs.nothing_to_change`）：成功回 204；沒連上 OBS 回 409 `obs.not_connected`；目前狀態或 OBS 找不到場景或物件回 404 `obs.not_found`；OBS 拒絕或送出時連線中斷回 502 `obs.request_failed`。場景頁屬於模組 `scene`，停用只隱藏它的頁面與功能磚。

## 6. Host 生命週期
Host 與 Tauri 同程序。關閉管理視窗只隱藏到系統匣，Overlay 繼續運作；關閉 port 修改小頁面也留在系統匣；從系統匣選「結束」才停止。

按 ✕ 關閉管理視窗時，Rust 一律 `prevent_close()`：`settings.json` 已存「不再提示」就直接隱藏；否則送事件給管理視窗顯示提示，前端用 app 指令 `close_hint` 回報「已顯示」「取消」或「隱藏」，「隱藏」時一併帶「不再提示」的勾選結果，由 Rust 寫入 `settings.json`。2 秒內沒收到「已顯示」，或提示中又按一次 ✕，就直接隱藏，避免按 ✕ 沒反應。設定頁「一般」的「關閉視窗前先詢問」改的是同一個值（管理 API `GET`／`PUT /api/admin/general`，`ask_before_close` 與「不再提示」相反）；Rust 每次按 ✕ 都重新讀取 `settings.json`，所以改了立即生效。

### 資料夾
每個資料夾是一個執行單位，以 `instance-id` 識別：設定、記錄檔、快取與 WebView2 資料都在裡面。程式只發可攜版，資料預設放在執行檔旁的 `data` 資料夾。
- 使用哪個資料夾：命令列 `--data-dir <路徑>` 指定的資料夾；沒有指定就用執行檔旁的 `data` 資料夾（dev 建置也一樣，所以在 `target\debug\data\`）。
- 內容：`settings.json`、`logs/`（JSON，按日輪替，保留 14 天）、`cache/`、`templates/`（`custom` 樣板，Host 不會建立）、`sounds/`（匯入的音效）、`music/`（匯入的背景音樂）、`media/`（兌換時播的影片與 GIF）、`checkin/`（集點卡樣式的圖片）、`WebView2/`、`instance-id`、`toolkit.db`（資料庫）、`backups/`（升級前與還原前的備份）；還原時另有暫時的 `restore-staging/`、`restore-pending` 與 `restore-previous/`（見下方「備份檔」）。
- 一個資料夾同時只跑一份；不同資料夾可以同時執行（例如測試新版）。`instance-id` 是資料夾裡的隨機 ID，搬移資料夾後不變；複製出來的資料夾被視為同一份，不能和原本的同時執行。只有 `instance-id` 不存在時才產生新的（`create_new`，同時啟動的兩份拿到同一個 ID）；其他讀取錯誤會顯示錯誤並結束，不會換成新 ID。
- Twitch 登入與 OBS 密碼依資料夾分開：Credential Manager 的項目名稱是 `<instance-id>/twitch` 與 `<instance-id>/obs`。
- 資料夾無法寫入時顯示錯誤並結束，不改用其他位置。
- 多份並存：解壓縮成多份，或用 `--data-dir` 指定不同資料夾。不出安裝檔（見 decisions.md「散布」）。

備份檔（`.stbackup`，設定頁的「備份與還原」）：
- zip 檔，內含 `settings.json`、資料庫 `toolkit.db`、`templates/`、`sounds/`、`music/`、`media/`、`checkin/` 與 `manifest.json`；不含 `logs/`、`cache/`、`WebView2/`、`backups/` 與 `instance-id`。Twitch 登入與 OBS 密碼在 Credential Manager，不在資料夾裡，所以不會進備份檔；Client ID 不是秘密，隨 `settings.json` 備份。
- 資料庫先用 `VACUUM INTO` 存成 `cache/` 裡的快照再放進 zip，不直接讀開啟中的檔案。直播中匯出時聊天室、特效與打卡照常運作，資料庫寫入只在快照期間稍等。
- `manifest.json`：`format`（`streamer-toolkit-backup`）、`format_version`（目前為 1）、`app_version`、`schema_version`（資料庫最新的 migration 版本，空的資料庫為 0）、`created_at`（UTC，RFC 3339）、`records`（`redemptions`、`checkins`：兌換紀錄與打卡紀錄的筆數）、`files`（每個檔案的 `path`、`size`、`sha256`）。
- 路徑只能是 `settings.json`、`toolkit.db`，或 `templates/`、`sounds/`、`music/`、`media/`、`checkin/` 底下的相對路徑，以 `/` 分隔；每一段不得為空、`.` 或 `..`，不得含 `\`、`:` 等 Windows 不允許的字元，也不得是保留名稱（`CON`、`NUL` 等）。符號連結與 junction 不備份。
- 上限：zip 512 MB、檔案 10,000 個、解壓縮後合計 2 GB。匯出超過上限時整份拒絕（413 `backup.too_large`，管理 UI 列出上限），不會略過任何檔案，所以匯出的檔案一定能還原。
- 匯出：管理 API `GET /api/admin/backup` 回傳備份檔。管理 UI 先開另存新檔對話方塊（`showSaveFilePicker`）選位置，再把回應寫進去；沒有這個 API 時改由瀏覽器下載。
- 還原只會整份取代，不合併：
  1. 選檔：管理 UI 把檔案原樣送到 `POST /api/admin/backup/restore/preview`（上限 512 MB，`DefaultBodyLimit`）。寫入任何東西之前，先在記憶體檢查上限、每個路徑、每個檔案的大小與 SHA-256（只讀到 manifest 寫的大小再多 1 byte，不相信 zip 自己記的大小）、zip 裡剛好是 manifest 列的檔案加上 manifest；`format_version` 或 `schema_version` 比這一版新時回 400 `backup.newer_version`，其他不符回 400 `backup.invalid`。通過後解到資料夾的 `restore-staging/`，再檢查資料庫本身（套用過的 migration 都必須是這一版認得、checksum 相同且已完成的；有不認得的也回 `backup.newer_version`）與 `settings.json` 讀得懂，最後才寫入 `manifest.json` 表示暫存完成，回傳預覽（備份時間、程式版本、兌換與打卡紀錄筆數、樣板資料夾數、音效檔數、背景音樂檔數）。任何一步失敗就刪掉 `restore-staging/`；使用中的資料都不動。
  2. 確認：`POST /api/admin/backup/restore`（沒有暫存時回 409 `backup.nothing_staged`）寫入 `restore-pending`（內容是這次還原的新 ID），回 202 後重新啟動程式（`AppHandle::request_restart`，沿用原本的命令列參數）。
  3. 啟動時（見「啟動順序」）：先把目前的資料存成 `backups/before-restore-<UTC 時間>.stbackup`（之後可以照樣還原回去）；再把目前的 `settings.json`、資料庫（含 `-wal`、`-shm`）、`templates/`、`sounds/`、`music/`、`media/`、`checkin/` 搬進 `restore-previous/`，把暫存的搬到原位（都在資料夾內改名；`restore-previous/` 在搬移前先寫入同一個還原 ID）；接著走一般的開資料庫流程（有待套用的 migration 時照樣先備份）。任一步失敗就把 `restore-previous/` 的東西放回原位，顯示原因後照常啟動；成功時顯示提醒：備份不含 Twitch 登入與 OBS 密碼，需要時重新登入、重填。最後刪掉 `restore-pending`、`restore-previous/`、`restore-staging/`；刪除前先刪掉還原 ID 與暫存的 `manifest.json`，所以 `restore-pending` 刪不掉時（顯示錯誤，請使用者手動刪除），下次啟動既不會放回舊資料，也不會再套用一次。
  - 程式在搬移途中結束時，下次啟動看到 `restore-pending` 與 `restore-previous/` 同時存在、且還原 ID 相同，就先放回原本的資料並說明還原中斷。ID 不同（或沒有 ID）的 `restore-previous/` 是先前已結束的還原沒清掉的，裡面的資料當時已備份到 `backups/`：絕不放回，套用新的還原前直接刪掉。連放回都失敗時保留兩者、顯示錯誤並結束，下次啟動再試。
  - 暫存後沒有確認：下次啟動直接刪掉 `restore-staging/`。
  - `instance-id` 不在備份裡，還原後不變；Credential Manager 的項目依它命名，所以還原到另一個資料夾或另一台電腦時要重新登入 Twitch、重填 OBS 密碼。
- 備份是匯出當下的快照：匯出後才被管理員刪除的聊天，原文仍留在那份備份裡；還原那份備份時原文會再出現，刪除不會跨還原保留。

啟動順序：
1. 決定資料夾並確認可寫入；`tauri-plugin-single-instance` 依 identifier 命名它的鎖，因此啟動時把 identifier 加上 `instance-id`，讓鎖依資料夾區分。先註冊這個 plugin，第二份程式在交棒前不會去搶 port。
2. 有已確認的還原就先套用（見「備份檔」），再開資料庫並套用 migration；兩者都在單一實例鎖之後，第二份程式不會動到同一個資料夾。
3. Host 綁 port；失敗時開啟 port 修改小頁面，使用者儲存後重試。
4. 正式版：為目前的 port 建立 remote capability，再建立管理視窗並載入 Host 網址。dev 模式：管理視窗載入 Vite dev server，使用 capability 2。
5. 建立任何視窗前先確認有 WebView2 Runtime（Windows 11 內建，少數 Windows 10 沒有）。缺少時顯示說明並開啟微軟的下載頁，Host、Overlay 與系統匣照常運作；安裝後在系統匣選「開啟」就會建立視窗，不必重新啟動。

## 7. 規則怎麼強制
- 跨模組相依性：由各 crate 的 `Cargo.toml` 強制。
- 層內規則：架構測試（`crates/arch-tests`）在 CI 執行，檢查 `domain/` 內沒有任何 `sqlx`／`axum` 字樣（含 `use`、derive、完整路徑）、`application/` 的程式碼（不含註解與測試）沒有用到 `infrastructure`（發布等轉接改由 infrastructure 注入 callback），並用 `cargo metadata` 檢查相依方向：模組只能依賴 `core` 與其他模組，不依賴 `integrations` 與 `host`；`core` 不依賴其他 workspace crate；`integrations` 只依賴 `core`；外部平台 SDK（`obws`、`tokio-tungstenite`）只有 `integrations` 可以依賴。模組的內部型別一律 `pub(crate)`，其他模組只看得到對方公開的介面；Cargo 本身禁止循環相依，出現循環時把共用型別移到 `core`。
