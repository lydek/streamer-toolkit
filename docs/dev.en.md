<!-- source: docs/dev.md source-sha256: c48d40437990e6e0e1aada203b4f9ecf858556f3c6da6c9096213f9b816d5a02 -->
[繁體中文](dev.md) | **English**

# Development

## Environment (Windows)
- Rust: version pinned by `rust-toolchain.toml`.
- Node: 24 LTS, in `.nvmrc`. Tauri CLI and lefthook are pnpm devDependencies, run with `pnpm tauri …` and `pnpm lefthook …`. pnpm: install once with `npm i -g pnpm`; it then switches versions according to `packageManager` in `package.json`, without Corepack; dependencies are locked by the lockfile; install with `pnpm install --frozen-lockfile`.
- Visual Studio Build Tools (MSVC) and WebView2 Runtime (required by Tauri).
- `cargo-deny`: `cargo install --locked cargo-deny` (the pre-push hook and CI both run `cargo deny check`).

Principle: another Windows PC can get it running by following this document.

## Common commands
- Install dependencies: `pnpm install --frozen-lockfile`
- Build the frontend: `pnpm web:build` (required before the first Rust build; the Host embeds `apps/web/dist` into the executable)
- Develop: `pnpm tauri dev` (Vite hot reload), or `cargo run -p desktop`
- API contracts: after changing the Host's HTTP API or WebSocket events, run `pnpm api:gen` (build the frontend first). Rust (`utoipa`) writes `apps/web/src/api/openapi.json`, then `openapi-typescript` writes `schema.d.ts`; commit both files. CI fails when regenerating changes them. The WebSocket snapshot and event types are also OpenAPI components, and tests compare them with the JSON actually sent; when you add an external event or a snapshot-retained event type, also add its type to the components in `crates/host/src/openapi.rs`.
- Checks: `pnpm web:lint`, `pnpm web:test`, `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo deny check`; to run the same steps as CI (also covering translations, the frontend build, API contracts and release clippy), use `pnpm lefthook run pre-push`
- Data: dev builds keep their data in `target\debug\data\` and never touch other copies' data; to test with another set of data, add `--data-dir <directory>` (the Vite proxy of `pnpm tauri dev` reads the port only from `target\debug\data\`, so that data's port must match it). Logs are in the data directory's `logs\` (JSON); use `RUST_LOG=debug` for more detail.

## Git
- The repo is public: anyone can see the code and commit history.
- Commit author email uses the GitHub noreply address: `git config user.email "<id>+<username>@users.noreply.github.com"` (this repo only).
- Branches: `main` is the default branch; each slice gets `feat/<slice>`, opens a PR, and is squash-merged after CI passes.
- Hooks: lefthook, configured in `lefthook.yml`, with `assert_lefthook_installed` on, so a missing lefthook fails the commit instead of silently skipping checks.
  - lefthook is a pnpm dev dependency; the `prepare` script in the root `package.json` runs `lefthook install`, so hooks are enabled after `pnpm install`, without relying on package postinstall scripts.
  - Hooks check the working tree, not the staged snapshot; CI runs the same check again on the pushed commit.
  - `pre-push` runs the full check in CI order (translations, frontend lint/test/build, API contracts up to date, `cargo fmt`, both `cargo clippy` runs, `cargo test`, `cargo deny check`) and blocks the push at the first failure, so failures surface before CI. Keep the `pre-push` steps in `lefthook.yml` in sync with `.github/workflows/ci.yml`; change one, change the other.
- Line endings are normalized by `.gitattributes` (LF; CRLF for Windows scripts).

## Before every commit (CI runs the same set, Windows only)
Order: build the frontend first (`dist/` is not under version control, and `rust-embed` needs it to compile), then run the Rust checks.
Formatting, lint, type checks, tests, architecture tests and `cargo deny check` (licenses and security advisories) all pass (Rust: `cargo fmt --check`, `cargo clippy`, `cargo test`; frontend: lint, typecheck, test, build); generated OpenAPI and TS types must be up to date.
Cargo commands without `tauri/custom-protocol` all compile in dev mode, so CI also runs `cargo clippy --features tauri/custom-protocol` to check code compiled only in release (for checking only; nothing is distributed). Releases always use `tauri build`.

## Testing baseline
- Test only the behavior we own, not Twitch/OBS themselves.
- Levels: unit tests for domain/use cases; integration tests for Repositories and HTTP against a real SQLite; contract checks that generated files are up to date. UI E2E deferred.
- Architecture tests (`crates/arch-tests`): `domain/` contains no occurrence of `sqlx`/`axum` at all; crate dependencies point the right way: modules depend only on the shared core (`crates/core`, package name `toolkit-core`) and other modules' public interfaces, never on `integrations` or `host`; modules do not use other modules' non-public types (rules in [architecture.en.md](architecture.en.md)).
- Migration test: merged versions are globally unique and sorted.
- Must test: redemption deduplication and state transitions, restoring a backup actually works, migrations lose no data.
- Test with simulated events instead of real Twitch triggers. Simulated events are tagged `source=simulation` and must never be written to any real business records (redemption records, check-in records).
- For actual OBS visuals and audio, one live-stream test per completed slice is sufficient.
- a11y baseline (checked manually once per finished slice): every action can be done with the keyboard, focus is visible, every control has a label, text contrast meets WCAG 2.2 AA (4.5:1), control boundaries and informative icons reach 3:1 (WCAG 1.4.11), and status is never conveyed by color alone.
  - How to check: walk through with the keyboard; compute contrast with the formula rather than judging by eye; use Emulate vision deficiencies in Edge/Chrome DevTools to confirm that under protanopia, deuteranopia and tritanopia, anything the colors no longer distinguish (such as green/yellow/red lights) can still be told apart by shape or text; with a 1280px-wide window zoomed to 200% and 400% (equivalent to 640px and 320px wide), there is no horizontal scrollbar, no text is clipped (except deliberate "…" truncation), and every button can be pressed. No screen reader testing, but semantic markup and ARIA are still done properly.
  - Drawer and dialog (`<dialog>`) behavior cannot be tested in jsdom, so walk through it manually in a real Tauri window:
    - After crossing the 1000px width threshold, focus, input values and scroll positions are preserved.
    - After typing and pressing Esc, the value has been saved.
    - When the drawer is modal, error messages and their buttons are visible and can be pressed.
    - With a dropdown open inside the drawer, Esc closes only the dropdown, not the drawer.
    - The close-window hint: Esc cancels and the window stays; "Got it" hides it to the tray; with "Don't show again" checked, the next close hides it to the tray directly.
  - Logic that can be extracted into pure functions still gets unit tests; the Rust close-hint state flow (shown/cancel/hide, the 2-second timeout, ✕ clicked again while the hint is open) gets Rust unit tests.
- Security test: `invoke` from the template preview iframe must be rejected.
- AI collaboration: may change implementation and add tests; must never loosen acceptance criteria, delete tests or change CI checks to make tests pass.

## Hard security and data constraints (never skip)
- HTTP/WebSocket bind to `127.0.0.1` only, never the LAN.
- The admin API requires the random token generated at every launch, delivered to the admin window over IPC and kept in memory only, never in localStorage; check the Host header and Origin; the admin window and the API share one origin, so there is no CORS; the admin window may only navigate to `/admin` paths, and `/admin` sends a strict CSP (details in [architecture.en.md](architecture.en.md)).
- Overlay routes are read-only, never get OAuth tokens, and cannot call the admin API.
- User templates and overlays are never rendered directly in the admin window; previews go in an iframe with only `sandbox="allow-scripts"` and receive data via `postMessage`.
- Escape all viewer input, such as chat messages and display names, to prevent XSS.
- Tokens are stored in Windows Credential Manager (one entry per data directory) and wrapped in `SecretString` in code; never in logs, plain files or backups.
- The repo is public: never commit secrets, tokens or personal data. Twitch uses the Device Code Flow, so no client secret is needed; each user registers their own Client ID and stores it in settings; it is never built into the repo.
- Redemption events are deduplicated by Twitch redemption ID.
- Back up the database automatically before migrations; no operation may silently delete data.
- Times are always stored in UTC; "daily" features (such as check-ins) use the time zone set by the streamer.

## Licenses
- This project is MIT. Rust dependency licenses must be on the `cargo-deny` allowlist (MIT, Apache-2.0, BSD-2/3-Clause, ISC, Zlib, Unicode-3.0, MPL-2.0, CDLA-Permissive-2.0).
- Check frontend dependencies with `pnpm licenses list`, only those shipped in the build; their allowlist also includes 0BSD, CC0-1.0, CC-BY-4.0, BlueOak-1.0.0, Python-2.0.
- Never add GPL/LGPL/AGPL.
- The portable package slice generates `THIRD_PARTY_LICENSES` (`cargo-about` + `pnpm licenses`), shipped in the zip.
- Bundled assets (sounds, fonts, icons) must have redistributable licenses (CC0, CC-BY, OFL, etc.), with source and license recorded in `assets/CREDITS.md`. Assets users import themselves never go into the repo.

## Documentation rules
- Maintain only the root `README.md`, `AGENTS.md`, and `scope.md`, `architecture.md`, `decisions.md`, `dev.md` under `docs/`.
- Complex modules (currently redemption records and daily check-in) get `docs/modules/<module>.md` when their slice starts, covering only rules, state transitions and acceptance; thin modules get none.
- Scope changes go into `scope.md`; layer or module rule changes go into `architecture.md`; tech choice changes get one line in the change log of `decisions.md`.
- To-dos and ideas become GitHub Issues, not documents.
- No ADRs, research documents or quality checklists. `docs/archive/` holds early drafts, is listed in `.gitignore`, exists only locally, and is reference only, not instructions.

## Language
- `README.md` and `docs/` (including `docs/modules/`) are bilingual: Traditional Chinese is the only authoritative version, and the English version is `<name>.en.md` in the same folder. `docs/archive/` is not translated. `AGENTS.md` is English only.
- Write Chinese only while working; English need not keep pace. **Before committing docs**, generate or update the English versions using an AI-assisted command, then have a human review them before committing.
- Line 1 of an English file records `<!-- source: <path to Chinese file> source-sha256: <hash> -->`, line 2 is the language switch (`[繁體中文](<name>.md) | **English**`); the Chinese file starts with `**繁體中文** | [English](<name>.en.md)`. Document links inside English files point to the matching `.en.md`.
- `node scripts/check-translations.mjs` checks three things: every Chinese file has an English version, every English file's source points to its same-name Chinese file (no orphans), and each English hash matches its Chinese file; otherwise it fails and prints the problem file or the new hash.
- Once git exists, the pre-commit hook runs this check and CI runs the same command; the hook only checks and never translates.
- Code: identifiers, comments, API/DB fields and error codes are always English. Error codes use `<area>.<reason>`, e.g. `twitch.sign_in_required`.
- Commits: English Conventional Commits, e.g. `feat(checkin): add daily check-in rule`.
- Fixed UI strings live in resource files, so translation later needs no component changes.
