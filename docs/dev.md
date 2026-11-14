**繁體中文** | [English](dev.en.md)

# 開發

## 環境（Windows）
- Rust：用 `rust-toolchain.toml` 鎖版本。
- Node：24 LTS，寫在 `.nvmrc`。Tauri CLI 與 lefthook 是 pnpm devDependencies，用 `pnpm tauri …`、`pnpm lefthook …` 執行。pnpm：用 `npm i -g pnpm` 安裝一次，之後依 `package.json` 的 `packageManager` 自動切換版本，不依賴 Corepack；用 lockfile 鎖定相依套件，安裝用 `pnpm install --frozen-lockfile`。
- Visual Studio Build Tools（MSVC）與 WebView2 Runtime（Tauri 需要）。
- `cargo-deny`：`cargo install --locked cargo-deny`（pre-push hook 與 CI 都會跑 `cargo deny check`）。

原則：另一台 Windows 電腦照這份文件就能跑起來。

## 常用指令
- 安裝相依套件：`pnpm install --frozen-lockfile`
- 建置前端：`pnpm web:build`（第一次編 Rust 前必須執行，Host 會把 `apps/web/dist` 內嵌進執行檔）
- 開發：`pnpm tauri dev`（Vite 熱更新），或 `cargo run -p desktop`
- API 契約：改了 Host 的 HTTP API 或 WebSocket 事件後執行 `pnpm api:gen`（需要先建置前端），由 Rust（`utoipa`）產生 `apps/web/src/api/openapi.json`，再由 `openapi-typescript` 產生 `schema.d.ts`，兩個檔案一起 commit；CI 重新產生後有差異就失敗。WebSocket 的快照與事件型別也放在 OpenAPI components，測試會拿實際送出的 JSON 比對；新增對外事件或快照保留的事件種類時，要一起在 `crates/host/src/openapi.rs` 的 components 加型別。
- 檢查：`pnpm web:lint`、`pnpm web:test`、`cargo fmt --all --check`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo test --workspace`、`cargo deny check`；要跑和 CI 相同的步驟（另含翻譯、前端建置、API 契約、正式版 clippy），用 `pnpm lefthook run pre-push`
- 資料：dev 建置的資料在 `target\debug\data\`，不會碰到其他份的資料；要用另一份資料測試，加 `--data-dir <資料夾>`。記錄檔在資料夾的 `logs\`（JSON），用 `RUST_LOG=debug` 看更多細節。

## Git
- Repo 是 public：任何人都看得到程式碼與 commit 紀錄。
- Commit 作者 email 用 GitHub noreply：`git config user.email "<id>+<帳號>@users.noreply.github.com"`（只設本 repo）。
- 分支：`main` 為預設分支；每個切片開 `feat/<切片>`，發 PR，CI 通過後 squash 合併。
- Hook：lefthook，設定在 `lefthook.yml`，已開啟 `assert_lefthook_installed`，找不到 lefthook 時 commit 會失敗，不會默默跳過檢查。
  - lefthook 是 pnpm 開發相依套件；根目錄 `package.json` 的 `prepare` script 執行 `lefthook install`，`pnpm install` 後自動啟用，不依賴套件的 postinstall。
  - Hook 檢查的是工作目錄，不是暫存區；CI 會在推上去的 commit 上再跑一次。
  - `pre-push` 依 CI 的順序跑完整檢查（翻譯、前端 lint／test／build、API 契約是否最新、`cargo fmt`、兩次 `cargo clippy`、`cargo test`、`cargo deny check`），任一步失敗就擋下 push，避免推上去才在 CI 失敗。`lefthook.yml` 的 `pre-push` 步驟要和 `.github/workflows/ci.yml` 保持一致，改一邊就要改另一邊。
- 換行由 `.gitattributes` 統一（LF；Windows 腳本 CRLF）。

## 每次提交前（CI 也跑同一套，只跑 Windows）
順序：先建置前端（`dist/` 不進版控，`rust-embed` 需要它才編得過），再跑 Rust 檢查。
格式、lint、型別檢查、測試、架構測試、`cargo deny check`（授權與安全公告）全部通過（Rust：`cargo fmt --check`、`cargo clippy`、`cargo test`；前端：lint、typecheck、test、build）；OpenAPI 與 TS 型別產物必須是最新。
沒有開啟 `tauri/custom-protocol` 的 cargo 指令都以 dev 模式編譯，所以 CI 另跑 `cargo clippy --features tauri/custom-protocol`，檢查只在正式版編譯的程式碼（只用來檢查，不散布產物）。正式發佈一律用 `tauri build`。

## 測試底線
- 只測我們負責的行為，不測 Twitch／OBS 本身。
- 層級：domain／use case 單元測試；Repository 與 HTTP 用真 SQLite 做整合測試；Contract 檢查產物是否最新。UI E2E 延後。
- 架構測試（`crates/arch-tests`）：`domain/` 內沒有任何 `sqlx`／`axum` 字樣；crate 相依方向正確：模組只依賴共用核心（`crates/core`，package 名稱 `toolkit-core`）與其他模組的公開介面，不依賴 `integrations` 與 `host`；模組不引用其他模組的非公開型別（規則見 [architecture.md](architecture.md)）。
- Migration 測試：合併後的版本全域唯一且已排序。
- 必測：兌換事件去重與狀態轉換、備份後能實際還原、migration 升級不掉資料。
- 用模擬事件測試，不必真的觸發 Twitch。模擬事件標記 `source=simulation`，不得寫入任何正式業務紀錄（兌換紀錄、打卡紀錄）。
- OBS 實際畫面與音效，每個切片完成時直播試用一次即可。
- a11y 基準（每個切片完成時人工檢查一次）：所有操作可用鍵盤完成、焦點可見、每個控制項有 label、文字對比達 WCAG 2.2 AA（4.5:1）、控制項邊界與傳達資訊的圖示達 3:1（WCAG 1.4.11）、狀態不只用顏色表達。
  - 檢查方法：用鍵盤走一遍；對比用公式實算，不用眼睛判斷；用 Edge／Chrome DevTools 的 Emulate vision deficiencies 看紅色盲、綠色盲、藍黃色盲下，顏色分不出來的地方（例如綠／黃／紅燈）仍能靠形狀或文字辨認；以 1280px 寬的視窗放大到 200%、400%（相當於 640px、320px 寬）時，沒有橫向捲軸、文字沒被裁掉（刻意以「……」省略的除外）、按鈕都按得到。不做螢幕閱讀器實測，但語意標記與 ARIA 照做。
  - 抽屜與對話方塊（`<dialog>`）的行為 jsdom 測不了，在實際 Tauri 視窗手動走一遍：
    - 跨過 1000px 寬度門檻後，焦點、輸入值、捲動位置都還在。
    - 打字後按 Esc，值已寫入。
    - 抽屜為 modal 時，錯誤訊息與其中的按鈕都看得到、按得到。
    - 抽屜內下拉選單開著時按 Esc，只關下拉選單、不關抽屜。
    - 關閉視窗時的提示：按 Esc 取消後視窗留著；按「知道了」後隱藏到系統匣；勾「不再提示」後，下次直接隱藏到系統匣。
  - 能抽成純函式的判斷照樣寫單元測試；關閉提示的 Rust 狀態流程（已顯示／取消／隱藏、2 秒逾時、提示中再按 ✕）寫 Rust 單元測試。
- 安全測試：樣板預覽 iframe 呼叫 `invoke` 必須被拒絕。
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
- 前端相依套件用 `pnpm licenses list` 檢查，只看會打包進產物的相依套件，允許清單另加 0BSD、CC0-1.0、CC-BY-4.0、BlueOak-1.0.0、Python-2.0。
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
