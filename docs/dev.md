**繁體中文** | [English](dev.en.md)

# 開發

## 環境（Windows）
- Rust：用 `rust-toolchain.toml` 鎖版本。
- Node：24 LTS，寫在 `.nvmrc`。Tauri CLI 與 lefthook 是 pnpm devDependencies，用 `pnpm tauri …`、`pnpm lefthook …` 執行。pnpm：用 `npm i -g pnpm` 安裝一次，之後依 `package.json` 的 `packageManager` 自動切換版本，不依賴 Corepack；用 lockfile 鎖定相依套件，安裝用 `pnpm install --frozen-lockfile`。
- Visual Studio Build Tools（MSVC）與 WebView2 Runtime（Tauri 需要）。
- `cargo-deny`：`cargo install --locked cargo-deny`（pre-push hook 與 CI 都會跑 `cargo deny check`）。
- Playwright 的 Chromium（UI E2E 用）：`pnpm --filter web exec playwright install chromium`，裝一次即可；瀏覽器放在使用者資料夾，不進 repo。
- sccache（可選）：讓所有 worktree 共用 Rust 相依套件的編譯結果，新 worktree 第一次編譯會比較快（同一份相依套件不必重編；各 worktree 自己的 crate 與 build script 產生的程式碼仍要編）。不裝也能編譯。
  - 安裝：`cargo install --locked sccache`，再在 PowerShell 執行 `[Environment]::SetEnvironmentVariable('RUSTC_WRAPPER', 'sccache', 'User')`，重開終端機後生效；用 `sccache --show-stats` 看命中率。
  - 只設在自己的環境變數，不寫進 repo 的 `.cargo/config.toml`。移除 sccache 前先在 PowerShell 執行 `[Environment]::SetEnvironmentVariable('RUSTC_WRAPPER', $null, 'User')` 並重開終端機，否則 cargo 會找不到它而編譯失敗。
  - 快取預設在 `%LOCALAPPDATA%\Mozilla\sccache`，上限 10 GB（`SCCACHE_CACHE_SIZE` 可改）。每個 worktree 仍有自己的 `target\`（dev 資料在 `target\debug\data\`），只共用快取。

原則：另一台 Windows 電腦照這份文件就能跑起來。

## 常用指令
- 安裝相依套件：`pnpm install --frozen-lockfile`
- 建置前端：`pnpm web:build`（第一次編 Rust 前必須執行，Host 會把 `apps/web/dist` 內嵌進執行檔）
- 開發：`pnpm tauri dev`（Vite 熱更新），或 `cargo run -p desktop`
- API 契約：改了 Host 的 HTTP API 或 WebSocket 事件後執行 `pnpm api:gen`（需要先建置前端），由 Rust（`utoipa`）產生 `apps/web/src/api/openapi.json`，再由 `openapi-typescript` 產生 `schema.d.ts`，兩個檔案一起 commit；CI 重新產生後有差異就失敗。WebSocket 的快照與事件型別也放在 OpenAPI components，測試會拿實際送出的 JSON 比對；新增對外事件或快照保留的事件種類時，要一起在 `crates/host/src/openapi.rs` 的 components 加型別。
- 檢查：`pnpm web:lint`、`pnpm web:test`、`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`、`pnpm web:e2e`、`cargo deny check`；要跑和 CI 相同的步驟（另含翻譯、前端建置、API 契約、正式版 clippy），用 `pnpm lefthook run pre-push --force`（不加 `--force` 會依推送的檔案跳過步驟，見下方「Git」）
- 資料：dev 建置的資料在 `target\debug\data\`，不會碰到其他份的資料；要用另一份資料測試，加 `--data-dir <資料夾>`（`pnpm tauri dev` 的 Vite proxy 只讀 `target\debug\data\` 的 port，所以那份資料的 port 要和它相同）。記錄檔在資料夾的 `logs\`（JSON），用 `RUST_LOG=debug` 看更多細節。

## Git
- Repo 是 public：任何人都看得到程式碼與 commit 紀錄。
- Commit 作者 email 用 GitHub noreply：`git config user.email "<id>+<帳號>@users.noreply.github.com"`（只設本 repo）。
- 分支與 PR：`main` 為預設分支；在分支上工作，發 PR，CI 通過後用 rebase-merge 合併（`gh pr merge <編號> --rebase`）。Repo 只開放 rebase-merge，`main` 保持一條直線。
  - 每個 issue 固定一個 Conventional Commit。push 前先在本機把同一個 issue 的工作併成一個 commit，PR 裡不留 fixup 或 review 修正的 commit：用 `git reset --soft`，或 `git commit --fixup` 再 `GIT_SEQUENCE_EDITOR=: git rebase -i --autosquash`（環境不支援互動式 rebase）。
  - 一個 issue 必須放得進一個 commit。範圍廣的 issue 改成母 issue，底下開 GitHub sub-issue：每個 sub-issue 一個 commit，母 issue 沒有 commit，所有 sub-issue 完成後關閉。做到一半發現 issue 太大時，先停下來向使用者提出拆分方式，不要把 commit 越做越大。
  - 一個 PR 可以放同一階段的多個獨立 issue，每個 issue 一個 commit，PR 說明逐一列出 `Closes #…`。會改到相同檔案的 issue 在同一個分支上依序做，不平行進行。
  - 分支要跟上 `main` 時，用 `git rebase origin/main` 再 `git push --force-with-lease`（只用在自己的分支）；不要把 `main` merge 進分支，merge commit 會讓 rebase-merge 失敗。
- Hook：lefthook，設定在 `lefthook.yml`，已開啟 `assert_lefthook_installed`，找不到 lefthook 時 commit 會失敗，不會默默跳過檢查。
  - lefthook 是 pnpm 開發相依套件；根目錄 `package.json` 的 `prepare` script 執行 `lefthook install`，`pnpm install` 後自動啟用，不依賴套件的 postinstall。
  - Hook 檢查的是工作目錄，不是暫存區；CI 會在推上去的 commit 上再跑一次。
  - `pre-push` 依 CI 的順序跑檢查（哪些步驟會跳過見下；翻譯、前端 lint／test／build、API 契約是否最新、`cargo fmt`、兩次 `cargo clippy`、`cargo test`、UI E2E、`cargo deny check`），任一步失敗就擋下 push，避免推上去才在 CI 失敗。`lefthook.yml` 的 `pre-push` 步驟要和 `.github/workflows/ci.yml` 保持一致，改一邊就要改另一邊。
  - `pre-push` 只跑這次推送的檔案會影響的步驟（CI 不跳過，每次都跑完整檢查）：
    - 只改文件（根目錄的 `*.md`、`docs/`）：只檢查翻譯；改到 `docs/decisions.md` 時另跑 script 測試與前端授權檢查（授權允許清單在這份文件裡）。
    - 只改前端（`apps/web/`）：跳過 `cargo fmt`、兩次 `cargo clippy` 與 `cargo deny check`，其餘照跑。只改管理 UI（`apps/web/src/admin/`）或 E2E（`apps/web/e2e/`）時也跳過 `cargo test`；改到前端其他地方時照跑，因為 Host 內嵌 `apps/web/dist`，Rust 測試也會檢查 Overlay 頁面與內建樣板（`apps/web/src/overlay/chat/templates/`）。
    - 改到其他任何檔案（Rust、`Cargo.*`、`lefthook.yml`、`.github/`、`package.json`、`pnpm-lock.yaml`、`rust-toolchain.toml`、`scripts/` 等）：全部照跑。
    - 比對對象是遠端分支（新分支則是 `origin` 的預設分支）；lefthook 不列出已經不存在的路徑（改名的檔案可能完全不出現），所以只刪除或改名檔案的推送可能跳過檢查，由 CI 把關。
- 換行由 `.gitattributes` 統一（LF；Windows 腳本 CRLF）。

## 每次提交前（CI 也跑同一套，只跑 Windows）
順序：先建置前端（`dist/` 不進版控，`rust-embed` 需要它才編得過），再跑 Rust 檢查。
格式、lint、型別檢查、測試、架構測試、UI E2E、`cargo deny check`（授權與安全公告）、`pnpm check:licenses`（前端授權）、`pnpm test:scripts`（`scripts/` 的 Node 測試）全部通過（Rust：`cargo fmt --check`、`cargo clippy`、`cargo test`；前端：lint、typecheck、test、build）；OpenAPI 與 TS 型別產物必須是最新。
沒有開啟 `tauri/custom-protocol` 的 cargo 指令都以 dev 模式編譯，所以 CI 另跑 `cargo clippy --features tauri/custom-protocol`，檢查只在正式版編譯的程式碼（只用來檢查，不散布產物）。正式發佈一律用 `tauri build`。

## 測試底線
- 只測我們負責的行為，不測 Twitch／OBS 本身。
- 層級：domain／use case 單元測試；Repository 與 HTTP 用真 SQLite 做整合測試；Contract 檢查產物是否最新；UI E2E 測 jsdom 測不了的瀏覽器行為。
- UI E2E（`apps/web/e2e/`，Playwright＋Chromium，`pnpm web:e2e`，需要先建置前端）：
  - 管理 UI 跑在 Vite dev server 上，管理 API 用 `page.route`、`/ws/overlay` 用 `page.routeWebSocket`、Tauri IPC 用 `e2e/fixtures.ts` 的假 `invoke` 代替；fixture 只放假的 token，不放任何真的 token 或 secret；Vite 送出與 Host 相同的 CSP，E2E 在這份 CSP 下執行（Host 實際送出的標頭由 `crates/host` 的 Rust 測試涵蓋）。還沒有實際功能用到的抽屜行為（設定欄位、下拉選單、「開啟完整頁」）在 `e2e/harness/` 的測試頁上測。
  - Overlay 對真的 Host 測：`crates/host/examples/e2e_host.rs` 在暫存資料夾啟動 Host（隨機管理 token、假的 secret 儲存、Twitch 與 OBS 都指向沒有服務的位址）。
  - 能用單元測試測的判斷照樣寫單元測試，E2E 不重複。
- 架構測試（`crates/arch-tests`）：`domain/` 內沒有任何 `sqlx`／`axum` 字樣；crate 相依方向正確：模組只依賴共用核心（`crates/core`，package 名稱 `toolkit-core`）與其他模組的公開介面，不依賴 `integrations` 與 `host`；模組不引用其他模組的非公開型別（規則見 [architecture.md](architecture.md)）。
- Migration 測試：合併後的版本全域唯一且已排序。
- 必測：兌換事件去重與狀態轉換、備份後能實際還原、migration 升級不掉資料。
- 用模擬事件測試，不必真的觸發 Twitch。模擬事件標記 `source=simulation`，不得寫入任何正式業務紀錄（兌換紀錄、打卡紀錄）。
- a11y 基準：所有操作可用鍵盤完成、焦點可見、每個控制項有 label、文字對比達 WCAG 2.2 AA（4.5:1）、控制項邊界與傳達資訊的圖示達 3:1（WCAG 1.4.11）、狀態不只用顏色表達。不做螢幕閱讀器實測，但語意標記與 ARIA 照做。
- 由 UI E2E 自動檢查（CI 每次都跑）：
  - 抽屜：視窗 > 1000px 時在功能磚旁並排（380px），功能磚仍可操作；≤ 1000px 時是右側 modal，點背景或按 Esc 會關；< 600px 時從底部出現。
  - 跨過 1000px 寬度門檻後，抽屜沒有關閉，焦點、輸入值、捲動位置都還在。
  - 打開抽屜時焦點在關閉鈕；關閉後焦點回到開啟它的功能磚、狀態燈或待處理項目（首頁還沒有待處理項目，用測試頁的替身測）；那個項目已不在時退回它所屬的功能磚（再往下退回第一個功能磚、「整理功能磚」的順序由單元測試涵蓋）。
  - 打字後不按 Enter，用下列方式離開抽屜，值都已寫入：✕、Esc、「開啟完整頁」（並排與 modal 各一次），再加並排時點另一個功能磚、modal 時點背景。
  - 儲存失敗時抽屜不關、欄位回到舊值並顯示錯誤；失敗發生在欄位失焦時，之後第一次按 ✕ 抽屜也不關（再按一次才會關）。
  - 抽屜為 modal 時，錯誤訊息與其中的按鈕都看得到、按得到；抽屜內下拉選單開著時按 Esc，只關下拉選單、不關抽屜。
  - 視窗寬 1280px、640px、320px（相當於 1280px 放大到 200%、400%）時沒有橫向捲軸；首頁的鍵盤路徑走得完：略過連結（焦點落在第一個功能磚）、開抽屜、送出模擬聊天、關抽屜（1280px、800px）。
  - 關閉提示的 `<dialog>`（IPC 用假的代替）：按 Esc 取消、按「知道了」、勾「不再提示」後送給 Rust 的動作都正確。
  - 整理功能磚：清空首頁後重新載入仍是空的，再從「整理功能磚」加回功能磚；進入、移除、加入、離開時的焦點；整理模式中打開抽屜（並排與 modal），第一次按 Esc 只關抽屜，再按一次才離開整理模式；1280px、640px、320px 沒有橫向捲軸；儲存失敗時功能磚回到原處並顯示錯誤。
  - 完整頁（⚙ 設定）：進入時焦點在「← 回首頁」；按它或上一頁（Alt+←）都回到首頁，且歷史紀錄沒有變長；回首頁後焦點回到 ⚙（從抽屜的「開啟完整頁」進入時回到功能磚，以及再往下的退回順序，由單元測試涵蓋），抽屜不會重新打開。
  - 設定頁的欄位打字後不按 Enter，直接按「← 回首頁」或上一頁離開，值都已寫入；儲存失敗時留在設定頁並顯示錯誤；視窗重新取得焦點時，重新讀取「關閉視窗前先詢問」。
  - 聊天室完整頁的樣板選擇器：點預覽就選定那一款並送出；方向鍵移動選擇；儲存失敗時退回原本的樣板並顯示錯誤；1280px、320px 沒有橫向捲軸。
  - Overlay（真的 Host）：選擇內建樣板後立即套用，不重新載入；改資料夾的自訂樣板（`custom`）後立即套用；樣板裡的 `<script>`、事件屬性與 `.js` 檔都不會執行；壞掉的樣板退回預設樣板。
- 每個切片完成時人工檢查一次：
  - 真的 OBS 連線與實際畫面、音效（直播試用一次）；動到 Twitch 登入或聊天的切片，另測真的 Twitch 登入與聊天。
  - 原生視窗 ✕ 與系統匣：按 ✕ 出現提示；按 Esc 取消後視窗留著；按「知道了」後隱藏到系統匣；勾「不再提示」後，下次直接隱藏到系統匣；設定頁開著時這樣隱藏視窗，再叫出視窗，「關閉視窗前先詢問」已變成未勾選。
  - 對比用公式實算，不用眼睛判斷；用 Edge／Chrome DevTools 的 Emulate vision deficiencies 看紅色盲、綠色盲、藍黃色盲下，顏色分不出來的地方（例如綠／黃／紅燈）仍能靠形狀或文字辨認；焦點看得見；放大到 200%、400% 時文字沒被裁掉（刻意以「……」省略的除外）、按鈕都按得到。
  - 整理功能磚：用滑鼠拖曳可在同一組內換位置，拖到另一組無效；< 600px 時功能磚排成單欄、按鈕自動換行；抽屜開著時按 ✎，先送出抽屜的變更再關抽屜。
  - 新頁面或新流程，以及上述路徑沒涵蓋的控制項（狀態燈、「整理功能磚」、其他功能磚），先用鍵盤走一遍，再補 E2E。
- 關閉提示的 Rust 狀態流程（已顯示／取消／隱藏、2 秒逾時、提示中再按 ✕）寫 Rust 單元測試。
- 安全測試：樣板預覽 iframe 呼叫 `invoke` 必須被拒絕。
  - 單元測試確認預覽 iframe 的 `sandbox` 只有 `allow-scripts`；Host 與 Vite dev server 的測試確認 `/admin` 的 `frame-src` 只放行預覽頁。
  - 實際拒絕要在 Tauri 視窗確認：預覽 iframe 裡仍有部分 `__TAURI_INTERNALS__`（Tauri 注入所有 frame 的 plugin 腳本），但它的 origin 是 `null`，呼叫 `invoke` 會被 Tauri 以 `Origin header is not a valid URL` 拒絕，對照組是管理視窗本身可以呼叫。檢查腳本：設 `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9333` 啟動程式並開啟聊天室完整頁（`/admin/chat`），執行 `node scripts/check-preview-ipc.mjs 9333`，腳本只確認被拒絕，不比對訊息；結束碼 0 才算通過。它需要實際的程式，所以不在 CI 裡跑；每次改動預覽、CSP 或 Tauri capability 時執行一次。
- AI 協作：可以改實作、加測試；不得為了讓測試通過而放寬驗收條件、刪除測試或修改 CI 檢查。

## 安全與資料硬約束（不可省略）
- HTTP／WebSocket 只綁 `127.0.0.1`，不開放區網。
- 管理 API 需要每次啟動產生的隨機 token，經 IPC 交給管理視窗、只放記憶體，不存 localStorage；檢查 Host 標頭與 Origin；管理視窗與 API 同源，不開 CORS；管理視窗只能導向 `/admin` 路徑，`/admin` 送嚴格 CSP（細節見 [architecture.md](architecture.md)）。
- Overlay 路由唯讀，拿不到 OAuth Token，也不能呼叫管理 API。
- 使用者樣板與 Overlay 絕不在管理視窗裡直接渲染；預覽放在只設 `sandbox="allow-scripts"` 的 iframe，資料用 `postMessage` 傳入。
- 聊天內容、暱稱等觀眾輸入一律 escape，防 XSS。
- Token 存在 Windows Credential Manager（項目依資料夾分開），程式內用 `SecretString` 包起來；不寫進記錄檔、明文檔案或備份檔。
- Repo 公開：不得 commit 任何 secret、Token 或個人資料。Twitch 用 Device Code Flow，不需要 client secret；Client ID 由每位使用者自己註冊並存在設定，不內建在 repo。
- 兌換事件依 Twitch 兌換 ID 去重。
- migration 前自動備份資料庫；任何操作都不能無聲刪除資料。
- 時間一律存 UTC；「每日」類功能（如打卡）依實況主設定的時區判斷。

## 授權
- 本專案為 MIT。Rust 相依套件的授權必須在 `cargo-deny` 允許清單內（MIT、Apache-2.0、BSD-2／3-Clause、ISC、Zlib、Unicode-3.0、MPL-2.0、CDLA-Permissive-2.0）。
- 前端相依套件由 `scripts/check-licenses.mjs`（`pnpm check:licenses`）檢查，CI 與 pre-push 都會跑：只看會打包進產物的相依套件（`pnpm licenses list --prod`，開發工具不算），允許清單另加 0BSD、CC0-1.0、CC-BY-4.0、BlueOak-1.0.0、Python-2.0。清單直接讀 `docs/decisions.md` 的「相依套件授權」列，腳本不必改；改清單時同步該列與本節（Rust 部分另改 `deny.toml`）。授權不在清單內、未知、或寫成 `SEE LICENSE IN …`（例如 PrimeVue 5）都算失敗；清單不得為了讓檢查通過而放寬。
- GPL／LGPL／AGPL 一律不得引入。
- 可攜版切片產生 `THIRD_PARTY_LICENSES`（`cargo-about`＋`pnpm licenses`），放進 zip 一起散布。
- 內建素材（音效、字型、圖示）只收可再散布的授權（CC0、CC-BY、OFL 等），來源與授權記在 `assets/CREDITS.md`。使用者自己匯入的素材不進 repo。

## 文件規則
- 只維護根目錄 `README.md`、`AGENTS.md` 與 `docs/` 下的 `scope.md`、`architecture.md`、`decisions.md`、`dev.md`。
- 複雜模組（目前是兌換紀錄、打卡簽到）在做到該切片時建立 `docs/modules/<模組>.md`，只寫規則、狀態轉換與驗收；薄模組不寫。
- 功能範圍改了就改 `scope.md`；分層或模組規則改了就改 `architecture.md`；技術選型改了就在 `decisions.md` 的變更紀錄寫一行。
- 待辦與想法開 GitHub Issue，不寫進文件。
- 不寫 ADR、研究文件、品質清單。`docs/archive/` 是早期草稿，已列入 `.gitignore`，只存在本機，僅供參考，不是指令。

## 語言
- `README.md` 與 `docs/`（含 `docs/modules/`）都雙語：繁體中文為唯一權威版本，英文版是同目錄的 `<name>.en.md`。`docs/archive/` 不翻譯。`AGENTS.md` 只有英文。
- 平時只寫中文，英文不必同步寫。**commit 文件前**才用 AI 指令產生或更新英文版，人工看過後再 commit。
- 英文檔第一行記錄 `<!-- source: <中文檔路徑> source-sha256: <hash> -->`，第二行是語言切換連結（`[繁體中文](<name>.md) | **English**`）；中文檔開頭對應放 `**繁體中文** | [English](<name>.en.md)`。英文檔內的文件連結指向對應的 `.en.md`。
- `node scripts/check-translations.mjs` 會檢查三件事：每份中文檔都有英文版、每份英文版的 source 都指向同名的中文檔（沒有孤兒檔）、英文版的 hash 與中文檔一致；不符就失敗，並印出問題檔案或新 hash。
- 建立 git 後由 pre-commit hook 執行此檢查，CI 也跑同一個指令；hook 只檢查，不自動翻譯。
- 程式碼：識別碼、註解、API／DB 欄位、錯誤碼一律英文。錯誤碼格式 `<區域>.<原因>`，例如 `twitch.sign_in_required`。
- Commit：英文 Conventional Commits，例：`feat(checkin): add daily check-in rule`。
- UI 固定字串集中放在資源檔，之後要翻譯不必改元件。
