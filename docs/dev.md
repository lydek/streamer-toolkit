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
- 檢查：`pnpm web:lint`、`pnpm web:test`、`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`、`cargo deny check`；UI E2E 另外跑（見下方「測試底線」）；要跑和 CI 相同的步驟（另含翻譯、script 測試、前端授權、前端建置、API 契約、正式版 clippy），用 `pnpm lefthook run pre-push --force`（不加 `--force` 會依推送的檔案跳過步驟，見下方「Git」）
- 資料：dev 建置的資料在 `target\debug\data\`，不會碰到其他份的資料；要用另一份資料測試，加 `--data-dir <資料夾>`，並把環境變數 `DEV_DATA_DIR` 設成同一個資料夾的絕對路徑（`pnpm tauri dev` 的 Vite proxy 從那裡讀 port；沒設就讀 `target\debug\data\`）。記錄檔在資料夾的 `logs\`（JSON），用 `RUST_LOG=debug` 看更多細節。
  - dev 建置在新資料夾預設用 port `47811`，正式版是 `47810`（OBS 的瀏覽器來源指向它），所以正式版沒開時，OBS 不會連到 dev 建置。已經存在的 `target\debug\data\settings.json` 存的 port 不會自動改，要改回 dev 的預設就手動改 `port` 或刪掉這個檔案。先寫好 `settings.json` 再啟動 dev 建置的腳本要自己寫入 `port`，否則會用正式版的預設。
  - dev 建置只開自己建立的資料夾：在 `target\debug\data\` 與新建或空的資料夾放 `dev-folder` 標記；`--data-dir` 指向不是空的、又沒有標記的資料夾（例如正式版的資料）時，什麼都不寫就結束，因為開發中的 migration 套用後，正式版就打不開那個資料庫。先寫好設定再啟動 dev 建置的腳本要一併建立 `dev-folder`。確定要用時加 `--allow-release-data`（每次都要加）；複製出來的正式版資料夾沿用同一個 `instance-id`，也就和原本那份共用 Credential Manager 裡的 token。
- 不連 OBS 與 Twitch 測試（假服務，`crates/fakes`）：
  - `pnpm dev:fake` 啟動假 OBS（`127.0.0.1:4466`）、假 Twitch（`127.0.0.1:4467`）與 dev 建置；資料夾是 `target\dev-fake-data\`（port `47812`），每次執行都重建，加 `--keep` 保留上一份。app 一啟動就登入假 Twitch、連上假 OBS。
  - 送事件：`cargo run -q -p fakes -- send <種類> [--user <login>] [--count <數量>] [--message <文字>] [--reward <獎勵名稱>] [--bits <數量>] [--tier 1000|2000|3000] [--months <月數>]`。種類：`follow`、`sub`、`resub`、`gift`（另外每位收禮者各一則 `sub`）、`cheer`、`raid`、`chat`（`--first` 加上首次聊天標記）、`delete`、`clear-user`、`clear`、`redeem`、`redeem-update`（`--id` 與 `--status fulfilled|canceled`）、`online`、`offline`。事件走真正的連線流程（`source=twitch`），會寫進那個資料夾的正式紀錄。
  - 其他指令：`obs drop`（OBS 斷線，app 會自己重連）、`obs stats [--cpu <%>] [--dropped <%>] [--kbps <數字>]`（效能頁以 60 秒視窗計算，掉幀調回 0 後要等視窗平均降回 5% 以下警告才消失）、`obs stream on|off`、`obs record on|off`、`eventsub drop`、`scenario stream-start|stream-end|gift-bomb|redemption-flood`。另一個 port 用 `--port <假 Twitch 的 port>`。
  - API 呼叫統計：`stats [--reset] [--json]` 列出 app 對兩個假服務的每種呼叫（Helix 路徑、OBS 請求、WebSocket 連線）：次數、每分鐘平均、10 秒內最高、平均與最短間隔、處理時間、同時進行數；每種事件之後的呼叫數與反應時間；Helix 每分鐘用量占 800 點的比例與 `/validate` 的頻率；以及異常警告（1.5 秒內重複、連續 5 次不到 1 秒的迴圈、一個事件引發超過 10 次呼叫、同時超過 4 個請求、重連沒有退避、缺標頭或 token 不對、超過一小時沒有檢查 token）。警告只列出來，不讓任何東西失敗。`--reset` 列出後歸零。門檻是 `crates/fakes/src/metrics.rs` 的常數。
  - 讓 API 失敗或變慢：`fault "<呼叫>" [--status <代碼>] [--delay <時間>] [--for <時間>] [--close]`，`fault --clear` 全部清除。呼叫的寫法和 `stats` 一樣（`GET /helix/users`、OBS 的請求名稱、`ws-connect eventsub|irc|obs`），時間像 `500ms`、`3s`、`2m`，預設持續 1 分鐘；`--close` 對連線會立刻斷開並拒絕新的連線。期間 `stats` 列出每次失敗後的重試次數與間隔，重試間隔沒有每次至少拉長 1.5 倍（到 55 秒為止）或超過 20 次時警告。
  - 壓力測試：先用假服務模式啟動 `e2e_host`（`E2E_ADMIN_TOKEN=<隨機> E2E_FAKE_SERVICES=127.0.0.1:4467 E2E_FAKE_OBS_PORT=4466 cargo run -p host --example e2e_host -- <port> <暫存資料夾>`）和 `fake-services`，再執行 `node scripts/bench.mjs <情境> --host 127.0.0.1:<port> --fake 127.0.0.1:4467 [--duration <秒>]`（`E2E_ADMIN_TOKEN` 同一個）。情境：`idle`（不送事件 10 分鐘，看 app 平常每分鐘呼叫幾次 API）、`chat`（每秒 50 則聊天）、`gifts`（一次 100 份禮物訂閱）、`redemptions`（200 次兌換）、`obs`（多 30 個來源每 50 毫秒送音量條、切 20 次場景、新增並移除 20 次來源）、`all`（聊天、兌換、OBS 負載同時）。報告寫到 `target\bench\`（Markdown 加 JSON）：聊天從送出到 Overlay 收到的延遲（中位數、p95、最大值）與遺失、特效佇列播完的時間、兌換紀錄是否都寫入、app 的 CPU 與記憶體、這段期間的 `stats`。不放進 CI，也不設門檻；改程式前後各跑一次比較。OBS 的音量條負載與突發變動也能用 `fake-services obs meters [--inputs <數量>] [--interval <毫秒>]`、`obs burst scenes|items [--count <次數>]` 手動觸發。
  - 只用假 OBS：`cargo run -p fakes`，在設定把 OBS 指向 `127.0.0.1:4466`；`--password <密碼>` 會要求驗證。
  - 開發版是靠 `TOOLKIT_FAKE_SERVICES=127.0.0.1:<port>` 改連假 Twitch（見 [architecture.md](architecture.md)「資料夾」）；`e2e_host` 用 `E2E_FAKE_SERVICES=127.0.0.1:<port>` 與 `E2E_FAKE_OBS_PORT=<port>`。

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
  - `pre-push` 依 CI 的順序跑檢查（哪些步驟會跳過見下；翻譯、script 測試、前端授權、前端 lint／test／build、API 契約是否最新、`cargo fmt`、兩次 `cargo clippy`、`cargo test`、`cargo deny check`；不跑 UI E2E），任一步失敗就擋下 push，避免推上去才在 CI 失敗。`lefthook.yml` 的 `pre-push` 步驟要和 `.github/workflows/ci.yml` 保持一致，改一邊就要改另一邊。
  - `pre-push` 只跑這次推送的檔案會影響的步驟（CI 只在 PR 只改文件、`prototype/`、`.agents/` 或 `.claude/` 時跳過，只檢查翻譯；改到 `docs/decisions.md` 或其他任何檔案都跑完整檢查）：
    - 只改文件（根目錄的 `*.md`、`docs/`）：只檢查翻譯；改到 `docs/decisions.md` 時另跑 script 測試與前端授權檢查（授權允許清單在這份文件裡）。
    - 只改前端（`apps/web/`）：跳過 `cargo fmt`、兩次 `cargo clippy` 與 `cargo deny check`，其餘照跑。只改管理 UI（`apps/web/src/admin/`）或 E2E（`apps/web/e2e/`）時也跳過 `cargo test`；改到前端其他地方時照跑，因為 Host 內嵌 `apps/web/dist`，Rust 測試也會檢查 Overlay 頁面與內建樣板（`apps/web/src/overlay/chat/templates/`、`apps/web/src/overlay/alerts/templates/`）。
    - 改到其他任何檔案（Rust、`Cargo.*`、`lefthook.yml`、`.github/`、`package.json`、`pnpm-lock.yaml`、`rust-toolchain.toml`、`scripts/` 等）：全部照跑。
    - 比對對象是遠端分支（新分支則是 `origin` 的預設分支）；lefthook 不列出已經不存在的路徑（改名的檔案可能完全不出現），所以只刪除或改名檔案的推送可能跳過檢查，由 CI 把關。
- 換行由 `.gitattributes` 統一（LF；Windows 腳本 CRLF）。

## 每次提交前（PR 的 CI 也跑同一套，只跑 Windows）
順序：先建置前端（`dist/` 不進版控，`rust-embed` 需要它才編得過），再跑 Rust 檢查。
CI 只在 PR 上跑；合併進 `main` 不再跑一次，因為 rebase-merge 合併的就是 PR 已檢查過的 commit。合併進 `main` 後，`cache.yml` 只編譯、不檢查，把 Rust 與 pnpm 快取存在 `main`；PR 從 `main` 還原快取，自己不存（各 PR 的快取會擠掉 `main` 的）。GitHub 更新 runner 映像的 Rust 後快取會對不上，到下次合併前可手動觸發 `cache.yml`。
格式、lint、型別檢查、測試、架構測試、`cargo deny check`（授權與安全公告）、`pnpm check:licenses`（前端授權）、`pnpm test:scripts`（`scripts/` 的 Node 測試）全部通過（Rust：`cargo fmt --check`、`cargo clippy`、`cargo test`；前端：lint、typecheck、test、build）；OpenAPI 與 TS 型別產物必須是最新。
沒有開啟 `tauri/custom-protocol` 的 cargo 指令都以 dev 模式編譯，所以 CI 另跑 `cargo clippy --features tauri/custom-protocol`，檢查只在正式版編譯的程式碼（只用來檢查，不散布產物）。正式發佈一律用 `tauri build`。

## 發佈
只出可攜版 zip。推 `v*` tag 後由 `.github/workflows/release.yml` 建置：
1. 先跑 UI E2E（`e2e.yml`），失敗就不發佈。
2. 建置 job 只有讀取權限，checkout 不保留 GitHub 憑證；先確認 tag 的 commit 在 `main` 上，再檢查 tag 和 `apps/desktop/tauri.conf.json`、`apps/desktop/Cargo.toml`、`package.json` 三處的版本一致（`node scripts/check-version.mjs <tag>`），不一致就不發佈。
3. 產生 `THIRD_PARTY_LICENSES`（見「授權」，`cargo-about` 固定版本），再執行 `pnpm tauri build --no-bundle`。
4. 打包 `streamer-toolkit-<版本>-windows-x64.zip`：裡面只有一個 `Streamer Toolkit\` 資料夾，放 `Streamer Toolkit.exe`、`LICENSE`、`THIRD_PARTY_LICENSES` 與 `使用說明.txt`（來源 `apps/desktop/release/usage.zh-TW.txt`），絕不含 `data`；旁邊附 `.sha256`。
5. 另一個 job 下載建置好的 zip 與 `.sha256`，建立草稿 release（`gh release create --draft`）。workflow 不會發佈；只有這個 job 有 `contents: write`，而且它不跑任何建置程式。

步驟：用 PR 把三處版本改成新版本並合併，在 `main` 上打 `v<版本>` tag 並推上去；workflow 完成後到 GitHub Releases 下載草稿的 zip，核對檢查碼，在沒裝開發工具的 Windows 上解壓縮並啟動，確認後才按發佈。

## 檔案長度
軟門檻，不進 CI。要不要拆看檔案有幾個改動的理由，行數只是提醒。
- 正式程式碼超過約 800 行、或測試檔超過約 1000 行時，下一個改到它的 issue 先評估要不要拆；要拆就另開 issue，不併進手上的 commit。
- 照責任拆（例如 `routes.rs`、`repo.rs`），不照行數硬切。
- 單一函式超過約 100 行（clippy `too_many_lines` 的預設值）時，同樣先評估：把可獨立命名的步驟或重複的準備抽成函式。測試函式走完一整個情境時可以更長，重複的準備步驟仍抽成 helper。
- Rust 測試太長時搬到 `tests` 子模組（`#[cfg(test)] mod tests;`，內容放 `<模組>/tests.rs`，或依主題放 `tests/<主題>.rs`），仍是 crate 內的單元測試，才能使用私有項目。子模組不要和 crate 根的模組或相依 crate 同名，否則會遮蔽 `use super::*` 帶進來的同名項目。

## 測試底線
- 只測我們負責的行為，不測 Twitch／OBS 本身。
- 層級：domain／use case 單元測試；Repository 與 HTTP 用真 SQLite 做整合測試；Contract 檢查產物是否最新；UI E2E 測 jsdom 測不了的瀏覽器行為。
- UI E2E（`apps/web/e2e/`，Playwright＋Chromium）：
  - 什麼時候跑：不在 PR 的 CI 與 pre-push 裡（它是最慢的一步）。在 GitHub Actions 手動觸發「UI E2E」workflow（`.github/workflows/e2e.yml`），發佈流程建置 zip 前也會先跑，失敗就不發佈。改到管理 UI、Overlay 或 E2E 時，push 前在本機跑一次。Rust 與前端的單元、整合測試仍是每個 PR 的必要檢查，因為它們守住資料規則。
  - 本機執行：先 `pnpm web:build`（Host 內嵌 `apps/web/dist`），再 `pnpm web:e2e`；第一次要先裝 Chromium（見上方「環境」）。
  - 管理 UI 跑在 Vite dev server 上，管理 API 用 `page.route`、`/ws/overlay` 用 `page.routeWebSocket`、Tauri IPC 用 `e2e/fixtures.ts` 的假 `invoke` 代替；fixture 只放假的 token，不放任何真的 token 或 secret；Vite 送出與 Host 相同的 CSP，E2E 在這份 CSP 下執行（Host 實際送出的標頭由 `crates/host` 的 Rust 測試涵蓋）。還沒有實際功能用到的抽屜行為（設定欄位、下拉選單、「開啟完整頁」）在 `e2e/harness/` 的測試頁上測。
  - Overlay 對真的 Host 測：`crates/host/examples/e2e_host.rs` 在暫存資料夾啟動 Host（隨機管理 token、假的 secret 儲存、Twitch 與 OBS 都指向沒有服務的位址）。這些測試共用同一個 Host，標上 `@host`，在 Playwright 的 `host` project 一次只跑一個；其他測試在 `admin` project 平行跑。
  - 能用單元測試測的判斷照樣寫單元測試，E2E 不重複。
- 架構測試（`crates/arch-tests`）：`domain/` 內沒有任何 `sqlx`／`axum` 字樣；`application/` 不使用 `infrastructure`；crate 相依方向正確：模組只依賴共用核心（`crates/core`，package 名稱 `toolkit-core`）與其他模組的公開介面，不依賴 `integrations` 與 `host`；模組不引用其他模組的非公開型別（規則見 [architecture.md](architecture.md)）。
- Migration 測試：合併後的版本全域唯一且已排序。
- 必測：兌換事件去重與狀態轉換、備份後能實際還原、migration 升級不掉資料。
- 用模擬事件測試，不必真的觸發 Twitch。模擬事件標記 `source=simulation`，不得寫入任何正式業務紀錄（兌換紀錄、打卡紀錄）。
- a11y 基準：所有操作可用鍵盤完成、焦點可見、每個控制項有 label、文字對比達 WCAG 2.2 AA（4.5:1）、控制項邊界與傳達資訊的圖示達 3:1（WCAG 1.4.11）、狀態不只用顏色表達。不做螢幕閱讀器實測，但語意標記與 ARIA 照做。
- 由 UI E2E 自動檢查（手動觸發與發佈前跑）：
  - 抽屜：視窗 > 1000px 時在功能磚旁並排（380px），功能磚仍可操作；≤ 1000px 時是右側 modal，點背景或按 Esc 會關；< 600px 時從底部出現。
  - 跨過 1000px 寬度門檻後，抽屜沒有關閉，焦點、輸入值、捲動位置都還在。
  - 打開抽屜時焦點在關閉鈕；關閉後焦點回到開啟它的功能磚、狀態燈或待處理項目（首頁還沒有待處理項目，用測試頁的替身測）；那個項目已不在時退回它所屬的功能磚（再往下退回第一個功能磚、「編輯首頁」的順序由單元測試涵蓋）。
  - 打字後不按 Enter，用下列方式離開抽屜，值都已寫入：✕、Esc、「開啟完整頁」（並排與 modal 各一次），再加並排時點另一個功能磚、modal 時點背景。
  - 儲存失敗時抽屜不關、欄位回到舊值並顯示錯誤；失敗發生在欄位失焦時，之後第一次按 ✕ 抽屜也不關（再按一次才會關）。
  - 抽屜為 modal 時，錯誤訊息與其中的按鈕都看得到、按得到；抽屜內下拉選單開著時按 Esc，只關下拉選單、不關抽屜。
  - 視窗寬 1280px、640px、320px（相當於 1280px 放大到 200%、400%）時沒有橫向捲軸；首頁的鍵盤路徑走得完：略過連結（焦點落在第一個功能磚）、開抽屜、送出模擬聊天、關抽屜（1280px、800px）。
  - 關閉提示的 `<dialog>`（IPC 用假的代替）：按 Esc 取消、按「知道了」、勾「不再提示」後送給 Rust 的動作都正確。
  - 編輯首頁：清空首頁後重新載入仍是空的，再從「編輯首頁」加回功能磚；進入、隱藏、加回、離開時的焦點；編輯模式中打開抽屜（並排與 modal），第一次按 Esc 只關抽屜，再按一次才離開編輯模式；1280px、640px、320px 沒有橫向捲軸；儲存失敗時功能磚回到原處並顯示錯誤。
  - 完整頁（⚙ 設定）：進入時焦點在頁首的第一個按鈕，沒有按鈕時在標題；按上方的「首頁」分頁或上一頁（Alt+←）都回到首頁，上一頁時歷史紀錄沒有變長；回首頁後焦點回到 ⚙（從抽屜的「開啟完整頁」進入時回到功能磚，以及再往下的退回順序，由單元測試涵蓋），抽屜不會重新打開。
  - 設定頁的欄位打字後不按 Enter，直接按「首頁」分頁或上一頁離開，值都已寫入；儲存失敗時留在設定頁並顯示錯誤；視窗重新取得焦點時，重新讀取「關閉視窗前先詢問」。
  - 聊天室完整頁的樣板選擇器：點預覽就選定那一款並送出；方向鍵移動選擇；儲存失敗時退回原本的樣板並顯示錯誤；1280px、320px 沒有橫向捲軸。
  - 特效模擬器：六種事件各送一次，送出的名稱、數量與留言正確；被拒絕時顯示原因；1280px、640px、320px 沒有橫向捲軸；真的 Host 把每種模擬事件都以 `alert` 送到 `/ws/overlay` 並標記 `source=simulation`，數量超出範圍回 400，沒有管理 token 回 401。
  - 特效 Overlay（真的 Host，播放時間縮短）：六種事件依送出順序一次播一則，每則都有音效，時間到就消失；送 3 份的送禮訂閱只播一則；切換內建樣板後立即換掉，不重新載入。
  - 集點卡 Overlay（真的 Host，顯示時間縮短）：模擬打卡依送出順序一次顯示一張，剛好集滿的有集滿效果，時間到就消失，且不寫入打卡紀錄；抽屜的打卡模擬器送出的名稱與格數正確。
  - 特效完整頁的樣板選擇器：每個預覽都播放範例特效；點預覽就選定並送出；方向鍵移動選擇；1280px、320px 沒有橫向捲軸。
  - 兌換紀錄：功能磚顯示最新一筆，抽屜列出最近幾筆，觀眾的文字不會被當成 HTML；完整頁送出的篩選與分頁正確，收到 `redemption.changed` 時用同樣的篩選與頁數重新讀取；沒有紀錄時顯示空白狀態；查詢被拒絕時顯示原因；1280px、640px、320px 沒有橫向捲軸。
  - 打卡：功能磚顯示今天的人數，抽屜列出最近幾位，觀眾名稱不會被當成 HTML，收到 `checkin.changed` 時更新；完整頁送出的搜尋與分頁正確，登記今天、補登過去某天送出的日期正確，重複時顯示提示；匯入先預覽，略過的項目列出原因（鍵當成文字顯示），確認後送出同一份內容，壞掉的 JSON 不送出；停用打卡前先詢問暫停或關閉，按 Esc 不變；1280px、640px、320px 沒有橫向捲軸。
  - 音效：功能磚顯示音效數量，抽屜與完整頁的名稱不會被當成 HTML；完整頁匯入不支援的檔案時顯示原因，超過 10 MB 的檔案不送出；改名與音量打字後不按 Enter，直接按「首頁」分頁離開，值也已寫入；刪除前先列出用到它的地方，取消後焦點回到「刪除」；1280px、640px、320px 沒有橫向捲軸；特效完整頁的六種事件可選內建音效、音效庫的音效或靜音，打卡設定可選一般打卡與集滿的音效，選了就送出；真的 Host 的模擬特效與集滿的模擬打卡，以選定的音效與音量播放，一般打卡不出聲；完整頁的頻道點數獎勵可選音效（Twitch 不再列出、但已選音效的獎勵以 ID 顯示，獎勵名稱不會被當成 HTML），「在 OBS 播放」送出選定的音效，送出後按鈕變成「停止播放」，按下就停止；真的 Host 的兌換音效 Overlay 依送出順序一次播一個，且不寫入兌換紀錄。
  - 備份：匯出先開另存新檔對話方塊（用假的代替）再寫入，取消就不匯出；沒有這個 API 時改由瀏覽器下載；被拒絕時顯示原因；還原先上傳選的檔案並顯示預覽（備份時間、程式版本、紀錄筆數、樣板與音效數），按下還原才送出確認並顯示正在重新啟動；損壞或較新版本的備份顯示原因、不提供還原；640px、320px 沒有橫向捲軸。
  - 聊天室 Overlay（真的 Host）：選擇內建樣板後立即套用，不重新載入；改資料夾的自訂樣板（`custom`）後立即套用；樣板裡的 `<script>`、事件屬性與 `.js` 檔都不會執行；壞掉的樣板退回預設樣板；斷線時狀態文字的顏色、字型、字級和陰影，與目前內建樣板的聊天文字相同（三款各測一次）。
- 每個切片完成時人工檢查一次：
  - 真的 OBS 連線與實際畫面、音效（直播試用一次）；動到 Twitch 登入或聊天的切片，另測真的 Twitch 登入與聊天。特效：OBS 瀏覽器來源勾選「透過 OBS 控制音訊」後，混音器出現該來源且聽得到音效；真的追隨、訂閱、小奇點、揪團各觸發一次。
  - 原生視窗 ✕ 與系統匣：按 ✕ 出現提示；按 Esc 取消後視窗留著；按「知道了」後隱藏到系統匣；勾「不再提示」後，下次直接隱藏到系統匣；設定頁開著時這樣隱藏視窗，再叫出視窗，「關閉視窗前先詢問」已變成未勾選。
  - 對比用公式實算，不用眼睛判斷；用 Edge／Chrome DevTools 的 Emulate vision deficiencies 看紅色盲、綠色盲、藍黃色盲下，顏色分不出來的地方（例如綠／黃／紅燈）仍能靠形狀或文字辨認；焦點看得見；放大到 200%、400% 時文字沒被裁掉（刻意以「……」省略的除外）、按鈕都按得到。
  - 編輯首頁：用滑鼠拖曳可換位置；< 600px 時功能磚排成單欄、按鈕自動換行；抽屜開著時按 ✎，先送出抽屜的變更再關抽屜。
  - 新頁面或新流程，以及上述路徑沒涵蓋的控制項（狀態燈、「編輯首頁」、其他功能磚），先用鍵盤走一遍，再補 E2E。
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
- 本專案為 MIT。Rust 相依套件的授權必須在 `cargo-deny` 允許清單內（MIT、Apache-2.0、BSD-2／3-Clause、ISC、Zlib、Unicode-3.0、MPL-2.0）。
- 前端相依套件由 `scripts/check-licenses.mjs`（`pnpm check:licenses`）檢查，CI 與 pre-push 都會跑：只看會打包進產物的相依套件（`pnpm licenses list --prod`，開發工具不算），允許清單另加 0BSD、CC0-1.0、CC-BY-4.0、BlueOak-1.0.0、Python-2.0。清單直接讀 `docs/decisions.md` 的「相依套件授權」列，腳本不必改；改清單時同步該列與本節（Rust 部分另改 `deny.toml`）。授權不在清單內、未知、或寫成 `SEE LICENSE IN …`（例如 PrimeVue 5）都算失敗；清單不得為了讓檢查通過而放寬。
- GPL／LGPL／AGPL 一律不得引入。
- 第三方授權清單 `THIRD_PARTY_LICENSES` 由 `scripts/third-party-licenses.mjs`（`pnpm licenses:gen`）產生：Rust 用 `cargo-about`（設定在 `scripts/about.toml`，只列執行檔用到的 crate），前端用 `pnpm licenses list --prod` 並附上各套件的授權檔，再加上 `assets/CREDITS.md` 的內建素材。只在發佈建置時產生、不 commit，所以永遠和當次建置的 lockfile 一致，新增套件後不必手動更新。產物寫到 `apps/web/public/THIRD_PARTY_LICENSES.txt`（已列入 `.gitignore`），前端建置時一起內嵌，設定頁「關於」以純文字顯示；同一份也放進 zip。dev 建置沒有這個檔案，「關於」會顯示載入失敗；要在本機產生，先 `cargo install --locked cargo-about --features cli`，再執行 `pnpm licenses:gen`。
- 內建素材（音效、字型、圖示）只收可再散布的授權（CC0、CC-BY、OFL 等），來源與授權記在 `assets/CREDITS.md`。使用者自己匯入的素材不進 repo。

## 文件規則
- 只維護根目錄 `README.md`、`AGENTS.md` 與 `docs/` 下的 `scope.md`、`architecture.md`、`decisions.md`、`dev.md`。
- 複雜模組（目前是兌換紀錄、打卡簽到）在做到該切片時建立 `docs/modules/<模組>.md`，只寫規則、狀態轉換與驗收；薄模組不寫。
- 功能範圍改了就改 `scope.md`；分層或模組規則改了就改 `architecture.md`；技術選型改了就在 `decisions.md` 的變更紀錄寫一行。
- 待辦與想法開 GitHub Issue，不寫進文件。
- 不寫 ADR、研究文件、品質清單。`docs/archive/` 是早期草稿，已列入 `.gitignore`，只存在本機，僅供參考，不是指令。
- 介面的目標是可點的 prototype `prototype/ui-v2.html`：進 git，每個分支都是同一份，裡面只放假資料。畫面要改先改它，確認後才拆 issue，改動和其他檔案一樣走 commit 與 PR。檢視時在 repo 根目錄執行 `python -m http.server 5180 --bind 127.0.0.1`，再開 `http://localhost:5180/prototype/ui-v2.html`；直接開檔案讀不到 `localStorage`，畫面會停在舊版面。圖示讀 `apps/web/node_modules/primeicons`，要先裝好相依套件。`main` 上的 `prototype/` 有改動時，`.github/workflows/pages.yml` 會自動部署到 GitHub Pages，不用 clone 也能看：`https://lydek.github.io/streamer-toolkit/`。

## 語言
- `README.md` 與 `docs/`（含 `docs/modules/`）都雙語：繁體中文為唯一權威版本，英文版是同目錄的 `<name>.en.md`。`docs/archive/` 不翻譯。`AGENTS.md` 只有英文。
- 平時只寫中文，英文不必同步寫。**commit 文件前**才用 AI 指令產生或更新英文版，人工看過後再 commit。
- 裝有 Codex CLI 時，可選擇在人工看之前先用它審查譯文。Codex 會把文件內容送到外部模型服務，所以 AI 代理要先問過使用者，得到同意才執行。中文定稿後只跑一次，之後中文意思有變才重跑：`codex exec -C . -s read-only --ephemeral -c model_reasoning_effort="medium" -o <報告檔> "<提示>" < /dev/null`。提示寫明權威檔與譯文路徑、只審未 commit 的差異、檢查新增、遺漏與語意偏移（特別是數字、否定、條件、術語）、不得改檔、用繁體中文回報 `檔案:行` 與可直接貼上的修正。每項先對照中文確認再改。
- 英文檔第一行記錄 `<!-- source: <中文檔路徑> source-sha256: <hash> -->`，第二行是語言切換連結（`[繁體中文](<name>.md) | **English**`）；中文檔開頭對應放 `**繁體中文** | [English](<name>.en.md)`。英文檔內的文件連結指向對應的 `.en.md`。
- `node scripts/check-translations.mjs` 會檢查三件事：每份中文檔都有英文版、每份英文版的 source 都指向同名的中文檔（沒有孤兒檔）、英文版的 hash 與中文檔一致；不符就失敗，並印出問題檔案或新 hash。
- pre-commit 與 pre-push hook 都會執行此檢查，CI 也跑同一個指令；hook 只檢查，不自動翻譯。
- 程式碼：識別碼、註解、API／DB 欄位、錯誤碼一律英文。錯誤碼格式 `<區域>.<原因>`，例如 `twitch.sign_in_required`。
- Commit：英文 Conventional Commits，例：`feat(checkin): add daily check-in rule`。
- UI 固定字串集中放在資源檔，之後要翻譯不必改元件。
