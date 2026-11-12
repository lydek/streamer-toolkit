<!-- source: docs/decisions.md source-sha256: 7455b0bd65361dbbbcbb509bf7d855c8f65820c12b72b3e3fd797b6d7e4831aa -->
[繁體中文](decisions.md) | **English**

# Technical decisions

Everything below is decided (2026-11-12). Change an item only when it causes a real problem, and add one line to the "Change log" at the bottom when you do. Layer and module rules are in [architecture.en.md](architecture.en.md).

## Choices
| Item | Decision |
|---|---|
| Architecture | DDD + Clean Architecture, implemented as a Modular Monolith |
| DDD scope | Tactical patterns only in complex modules; criteria in architecture.en.md |
| Module layout | Cargo workspace, one crate per module, with `domain`/`application`/`infrastructure` folders inside; thin modules keep all three layers too |
| Module communication | Public application service traits + domain events (through the outbox); integration events such as chat are only broadcast in memory |
| Data ownership | One SQLite file, tables owned by modules, no cross-module joins; write transactions always use `BEGIN IMMEDIATE` (or a single writer connection) |
| Model separation | Complex modules: Domain/DTO/Row/Event fully separate; thin modules: only DTOs split out |
| Backend | Rust + Axum |
| Desktop/frontend | Tauri 2 + Vue 3 + TypeScript |
| Database access | SQLite + SQLx, schema managed by migrations |
| Communication | All clients use HTTP (commands/queries) + WebSocket (events), with snapshot + resync on reconnect; the exception is a few Tauri IPC commands (see architecture.en.md) |
| API contract | Code-first: generate OpenAPI and TS types from Rust |
| Event format | Envelope from the first version: `id`, `type`, `version`, `seq`, `occurredAt`, `source` |
| Host lifecycle | Same process as Tauri; closing the window hides it to the system tray; single instance |
| Architecture enforcement | Crate dependencies + architecture tests (run in CI) |
| Testing | First version: Unit + Integration (real SQLite) + Contract (generated files up to date); UI E2E deferred |
| CI | GitHub Actions, built with the skeleton, Windows only; also runs `cargo clippy --features tauri/custom-protocol` to check code compiled only in release |
| Releases | Always built with `tauri build`; never distribute builds without `tauri/custom-protocol` |
| Overlay customization | First version allows HTML/CSS templates only |
| Platforms | Windows first; macOS after the MVP; Linux not supported |
| Decision records | This file + architecture.en.md; no ADRs |
| Document structure | README + four files under `docs/` (scope/architecture/decisions/dev); complex modules also get `docs/modules/<module>.md` |
| Document language | README and `docs/` are bilingual: Traditional Chinese is authoritative, the English version is the matching `.en.md`; write Chinese only while working and generate English before committing docs; sync is checked by hash (rules in dev.en.md). Archived documents are not translated |
| Code language conventions | Identifiers, comments, API/DB fields and error codes are always English |
| Commits | English Conventional Commits, e.g. `feat(checkin): add daily check-in rule` |
| Backlog | GitHub Issues (labels for MVP/later/bug), not in documents |
| AI guide | `AGENTS.md` (English); `CLAUDE.md` only references `AGENTS.md` |
| Remote repo | GitHub public |
| License | MIT (`LICENSE`) |
| Dependency licenses | Rust (`cargo-deny`) allowlist: MIT, Apache-2.0, BSD-2/3-Clause, ISC, Zlib, Unicode-3.0, MPL-2.0, CDLA-Permissive-2.0. The frontend has its own list and checks only dependencies shipped in the build: the above plus 0BSD, CC0-1.0, CC-BY-4.0, BlueOak-1.0.0, Python-2.0. GPL/LGPL/AGPL always blocked |
| Third-party license list | `cargo-about` (Rust) + `pnpm licenses` (frontend) combined into `THIRD_PARTY_LICENSES`, shipped in the installer and the About screen |
| Bundled assets | Only redistributable assets (CC0, CC-BY, OFL for fonts, etc.), with source and license recorded in `assets/CREDITS.md`; user-imported assets never go into the repo |
| Branches | Default `main`; each slice gets a short-lived `feat/<slice>` branch, squash-merged by PR after CI passes |
| Line endings | `.gitattributes`: LF everywhere; CRLF for `.ps1`/`.bat`/`.cmd`; images, sounds, videos and fonts marked binary |
| Git hooks | lefthook (`lefthook.yml`) |
| Commit identity | GitHub noreply email, set for this repo only |
| Twitch OAuth | Device Code Flow (public client, no client secret needed) |
| Twitch app | Each user registers their own app (Public client; enter `http://localhost` for the Redirect URL the form requires) in the Twitch Console; first-run setup guides them through registering the app and entering the Client ID, which is stored in settings; no shared built-in Client ID |
| Twitch scopes | Request all MVP scopes at first login: `user:read:chat`, `moderator:read:followers`, `channel:read:subscriptions`, `channel:read:redemptions` |
| Token refresh | Only one refresh at a time; write the new refresh token to keyring before using the new access token, and if that write fails, keep the new tokens in memory and retry the write (if the app closes before the retry succeeds, the next start requires logging in again); if refresh fails (400/401), restart the Device Code Flow and show "login required" in the UI. Public-client refresh tokens expire after 30 days, so long inactivity requires logging in again |
| Admin window | Loads the Host URL `http://127.0.0.1:<port>/admin` (same origin as the API, no CORS); may only navigate to `/admin` paths, and opening new windows is always denied; `/admin` sends a strict CSP; the Twitch verification page is opened in the system browser by a Rust command |
| Tauri IPC permissions | App commands declared with `AppManifest`; three capabilities: small port-change page (`port` window only), admin window dev (local, `admin` window only), admin window release build (remote, created at startup for the actual port, URLs limited to `/admin*`); "dev" means the `tauri/custom-protocol` feature is off (`cfg(dev)`): `tauri build` turns it on and is the only distributable build; `tauri dev` and plain cargo commands are dev; `apps/desktop` decides and tells the Host through runtime configuration |
| Local API security | Per-launch random admin token, delivered to the admin window over an IPC command and kept in memory only; Host header allowlist; Origin checks (`Origin: null` rejected); overlays are read-only, carry no token, and every response on overlay and template paths sends `script-src 'self'; object-src 'none'`; the template directory serves only non-script files with `nosniff`; template previews go in a sandboxed iframe and receive data via `postMessage` (details in architecture.en.md) |
| Port | Fixed default port `47810`; if taken, never switch ports automatically; instead open the small port-change page where the user changes it and retries |
| Frontend files | Admin UI and overlays embedded into the executable with `rust-embed`, served by the Host; the small port-change page serves as Tauri's `frontendDist`; `app.windows: []`, with all windows created from Rust (in `setup` or after a successful port retry); the build `assets/` directory and the template directory send `Access-Control-Allow-Origin: *` so the preview iframe can load them |
| Dev mode | Vite dev server + proxy (`/api`, `/ws`, target `http://127.0.0.1:<port>`, with `changeOrigin: true` and `ws: true`) to the Host; Tauri `build.devUrl` set to the Vite URL, Vite `server.strictPort: true`; Vite sends the same CSP as `/admin`; in dev mode Vite allows the `null` origin on everything except `/api` and `/ws` (`server.cors: false` + middleware); dev mode adds the Vite origin to the Origin allowlist |

## Packages
| Purpose | Package |
|---|---|
| Domain event dispatch | Custom outbox dispatcher: events are written to the outbox table in the same transaction as the business data, dispatched in the background with delivery state tracked per subscriber, retried on failure and shown in the UI, and resent after a restart; when disabling a module the streamer chooses "pause" (events kept and delivered after re-enabling) or "off" (after a warning, events from the disabled period skipped); events past the retry cap move to a dead-letter queue; fully delivered events are cleaned up periodically; subscribers deduplicate by event ID with an inbox table (details in architecture.en.md) |
| Migrations | Each module has its own folder, files use timestamp versions (globally unique); `host` merges them into a single SQLx Migrator at startup; rules and dev command in architecture.en.md |
| Error handling | `thiserror` (domain/application, mapped to error codes) + `anyhow` (only in the host composition root and startup) |
| Time | `chrono` + `chrono-tz` |
| IDs | UUID v7 (`uuid`); external IDs (e.g. Twitch redemption IDs) stored in separate columns |
| Twitch | Hand-written serde types (only the Helix, OAuth and EventSub fields we use) + custom EventSub WebSocket connection (`tokio-tungstenite`); chat via EventSub `channel.chat.message` |
| OBS | `obws`; connected in the MVP, starting with OBS connection status |
| Credentials | `keyring`, pinned to a major version with the Windows native store feature enabled (never falling back to the mock store) |
| Secrets | `secrecy`: tokens are wrapped in `SecretString` and never logged |
| Logging | `tracing` + `tracing-subscriber` (JSON) + `tracing-appender` (daily rotation with a `max_log_files` retention limit); output-layer masking is only a backup |
| Embedded frontend | `rust-embed` (with `debug-embed`) |
| OpenAPI | `utoipa` |
| Frontend API types | `openapi-typescript` + `openapi-fetch`; WS event types also live in OpenAPI components |
| Architecture tests | Custom Rust test: `cargo_metadata` checks crate dependencies and scans `domain/` for any occurrence of `sqlx`/`axum` (including derives and full paths) |
| Frontend structure | `apps/web` as one Vite project with multiple entries (admin UI + each overlay + small port-change page); overlays do not load the UI component library |
| UI component library | PrimeVue |
| Frontend state | Pinia |
| Frontend tests | Vitest + Vue Test Utils |
| Single instance | `tauri-plugin-single-instance` |
| Opening external URLs | `tauri-plugin-opener`, called from a Rust command (opens the Twitch verification page in the system browser) |
| Installer | NSIS (Tauri bundler) |
| Dev tools | `@tauri-apps/cli` and `lefthook` are pnpm devDependencies, version-locked by the lockfile, with no global install |
| Package management | Node 24 LTS (in `.nvmrc`); install pnpm once with `npm i -g pnpm`, after which it switches versions according to `packageManager` in `package.json`; no Corepack |

## Adoption timing
| When | Items |
|---|---|
| **Build with the skeleton** | Module enable/disable switch (data kept when disabled; in-memory events dropped while disabled), small port-change page shown when the port is taken, automatic backup before migrations, structured logs + error codes, event simulator (`source=simulation`), data directory layout (program/user data/cache/credentials kept apart), UI string resource keys (vue-i18n, `zh-TW` only at first), a11y baseline (defined in dev.en.md), tokens in Windows Credential Manager, CI, architecture tests, `cargo-deny` license and security advisory checks, local API security (admin token, Host and Origin checks, no CORS on the API), Twitch app registration guide and Device Code Flow login, automatic token refresh |
| **When the slice needs it** | Outbox and the "pause/off" choice when disabling (redemption records slice), portable backup format `.stbackup` (backup and restore slice), release workflow and `THIRD_PARTY_LICENSES` (installer slice) |
| **After the MVP** | OBS Dock, macOS installer, PostgreSQL/NAS storage (Storage Port), plugin SDK, mutation testing, second language `en`, remote/mobile control, auto-update, sandbox for custom overlay JS, diagnostic report export, dark/light theme and DPI checks, UI E2E |
| **Not doing** | Lightweight harness (only the "never loosen acceptance criteria" rule is kept, in dev.en.md), microservices, GraphQL/gRPC, WebSocket RPC, general rules engine, Linux, native mobile apps |

## Change log
| Date | What changed | Why |
|---|---|---|
| 2026-11-13 | Node 26 LTS changed to Node 24 LTS | Already installed on the dev machine and supported until 2028-04; pnpm manages its own version without Corepack, so 26's advantages do not matter for this project |
| 2026-11-13 | Tauri CLI and lefthook changed to pnpm devDependencies | Versions locked by the lockfile, with no system changes |
| 2026-11-13 | Twitch types changed from the `twitch_api` crate to hand-written serde types | The MVP uses only a few events; drops a large dependency tree and gives full control over fields and values such as `unknown` |
