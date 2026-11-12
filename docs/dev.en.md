<!-- source: docs/dev.md source-sha256: f7e56f3cfbe29142b26253790cd1328a78c6c884c675adca266de9b5615ff863 -->
[繁體中文](dev.md) | **English**

# Development

## Environment (Windows)
- Rust: version pinned by `rust-toolchain.toml`.
- Node: 26 LTS, in `.nvmrc`. pnpm: install once with `npm i -g pnpm`; it then switches versions according to `packageManager` in `package.json`, without Corepack; dependencies are locked by the lockfile; install with `pnpm install --frozen-lockfile`.
- Visual Studio Build Tools (MSVC) and WebView2 Runtime (required by Tauri).

Exact commands will be added once the walking skeleton exists. Principle: another Windows PC can get it running by following this document.

## Git
- The repo is public: anyone can see the code and commit history.
- Commit author email uses the GitHub noreply address: `git config user.email "<id>+<username>@users.noreply.github.com"` (this repo only).
- Branches: `main` is the default branch; each slice gets `feat/<slice>`, opens a PR, and is squash-merged after CI passes.
- Hooks: lefthook, configured in `lefthook.yml`, with `assert_lefthook_installed` on, so a missing lefthook fails the commit instead of silently skipping checks.
  - Before the frontend project exists: install lefthook globally (`scoop install lefthook` or `npm i -g lefthook`), then run `lefthook install`.
  - After the frontend project exists: lefthook becomes a pnpm dev dependency, and the `prepare` script in `package.json` runs `lefthook install`, without relying on package postinstall scripts.
  - Hooks check the working tree, not the staged snapshot; CI runs the same check again on the pushed commit.
- Line endings are normalized by `.gitattributes` (LF; CRLF for Windows scripts).

## Before every commit (CI runs the same set, Windows only)
Order: build the frontend first (`dist/` is not under version control, and `rust-embed` needs it to compile), then run the Rust checks.
Formatting, lint, type checks, tests, architecture tests and `cargo deny check` (licenses and security advisories) all pass (Rust: `cargo fmt --check`, `cargo clippy`, `cargo test`; frontend: lint, typecheck, test, build); generated OpenAPI and TS types must be up to date.
Cargo commands without `tauri/custom-protocol` all compile in dev mode, so CI also runs `cargo clippy --features tauri/custom-protocol` to check code compiled only in release (for checking only; nothing is distributed). Releases always use `tauri build`.

## Testing baseline
- Test only the behavior we own, not Twitch/OBS themselves.
- Levels: unit tests for domain/use cases; integration tests for Repositories and HTTP against a real SQLite; contract checks that generated files are up to date. UI E2E deferred.
- Architecture tests: `domain/` contains no occurrence of `sqlx`/`axum` at all; modules do not use other modules' non-public types (rules in [architecture.en.md](architecture.en.md)).
- Migration test: merged versions are globally unique and sorted.
- Must test: redemption deduplication and state transitions, restoring a backup actually works, migrations lose no data.
- Test with simulated events instead of real Twitch triggers. Simulated events are tagged `source=simulation` and must never be written to any real business records (redemption records, check-in records).
- Real OBS visuals and sound: one live-stream trial per finished slice is enough.
- a11y baseline (checked manually once per finished slice): every action can be done with the keyboard, focus is visible, every control has a label, text contrast meets WCAG 2.2 AA (4.5:1), and status is never conveyed by color alone.
- Security test: `invoke` from the template preview iframe must be rejected.
- AI collaboration: may change implementation and add tests; must never loosen acceptance criteria, delete tests or change CI checks to make tests pass.

## Hard security and data constraints (never skip)
- HTTP/WebSocket bind to `127.0.0.1` only, never the LAN.
- The admin API requires the random token generated at every launch, delivered to the admin window over IPC and kept in memory only, never in localStorage; check the Host header and Origin; the admin window and the API share one origin, so there is no CORS; the admin window may only navigate to `/admin` paths, and `/admin` sends a strict CSP (details in [architecture.en.md](architecture.en.md)).
- Overlay routes are read-only, never get OAuth tokens, and cannot call the admin API.
- User templates and overlays are never rendered directly in the admin window; previews go in an iframe with only `sandbox="allow-scripts"` and receive data via `postMessage`.
- Escape all viewer input, such as chat messages and display names, to prevent XSS.
- Tokens are stored in Windows Credential Manager and wrapped in `SecretString` in code; never in logs, plain files or backups.
- The repo is public: never commit secrets, tokens or personal data. Twitch uses the Device Code Flow, so no client secret is needed; each user registers their own Client ID and stores it in settings; it is never built into the repo.
- Redemption events are deduplicated by Twitch redemption ID.
- Back up the database automatically before migrations; no operation may silently delete data.
- Times are always stored in UTC; "daily" features (such as check-ins) use the time zone set by the streamer.

## Licenses
- This project is MIT. Rust dependency licenses must be on the `cargo-deny` allowlist (MIT, Apache-2.0, BSD-2/3-Clause, ISC, Zlib, Unicode-3.0, MPL-2.0, CDLA-Permissive-2.0).
- Check frontend dependencies with `pnpm licenses list`, only those shipped in the build; their allowlist also includes 0BSD, CC0-1.0, CC-BY-4.0, BlueOak-1.0.0, Python-2.0.
- Never add GPL/LGPL/AGPL.
- The installer slice generates `THIRD_PARTY_LICENSES` (`cargo-about` + `pnpm licenses`), shipped with the installer.
- Bundled assets (sounds, fonts, icons) must have redistributable licenses (CC0, CC-BY, OFL, etc.), with source and license recorded in `assets/CREDITS.md`. Assets users import themselves never go into the repo.

## Documentation rules
- Maintain only the root `README.md`, `AGENTS.md`, and `scope.md`, `architecture.md`, `decisions.md`, `dev.md` under `docs/`.
- Complex modules (currently redemption records and daily check-in) get `docs/modules/<module>.md` when their slice starts, covering only rules, state transitions and acceptance; thin modules get none.
- Scope changes go into `scope.md`; layer or module rule changes go into `architecture.md`; tech choice changes get one line in the change log of `decisions.md`.
- To-dos and ideas become GitHub Issues, not documents.
- No ADRs, research documents or quality checklists. `docs/archive/` holds early drafts, is listed in `.gitignore`, exists only locally, and is reference only, not instructions.

## Language
- `README.md` and `docs/` (including `docs/modules/`) are bilingual: Traditional Chinese is the only authoritative version, and the English version is `<name>.en.md` in the same folder. `docs/archive/` is not translated. `AGENTS.md` is English only.
- Write Chinese only while working; English need not keep pace. **Before committing docs**, generate or update the English versions with an AI command, have a person review them, then commit.
- Line 1 of an English file records `<!-- source: <path to Chinese file> source-sha256: <hash> -->`, line 2 is the language switch (`[繁體中文](<name>.md) | **English**`); the Chinese file starts with `**繁體中文** | [English](<name>.en.md)`. Document links inside English files point to the matching `.en.md`.
- `node scripts/check-translations.mjs` checks three things: every Chinese file has an English version, every English file's source points to its same-name Chinese file (no orphans), and each English hash matches its Chinese file; otherwise it fails and prints the problem file or the new hash.
- Once git exists, the pre-commit hook runs this check and CI runs the same command; the hook only checks and never translates.
- Code: identifiers, comments, API/DB fields and error codes are always English.
- Commits: English Conventional Commits, e.g. `feat(checkin): add daily check-in rule`.
- Fixed UI strings live in resource files, so translation later needs no component changes.
