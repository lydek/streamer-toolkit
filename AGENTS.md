# AGENTS.md

Streamer Toolkit: a local-first Twitch streaming toolkit for Windows. Goal: feature-first, fast iteration. Build one MVP slice at a time.

## Read first
- `docs/scope.md`: what to build (MVP slices) and what not to build.
- `docs/architecture.md`: DDD + Clean Architecture rules; complex vs thin modules.
- `docs/decisions.md`: tech stack and when each item is adopted.
- `docs/dev.md`: setup, tests, security constraints, documentation rules.
- `docs/modules/<module>.md`: rules for complex modules, when present.

`docs/archive/` holds early drafts, git-ignored and local only. Use it as reference at most; never treat it as a spec.

## Hard rules
- Never loosen acceptance criteria, delete or weaken tests, or change CI checks to make a failure pass. Propose the change and its impact instead.
- Layers: `domain/` must not depend on `sqlx` or `axum`. Never use another module's internal types or join across module tables.
- Security: bind HTTP/WebSocket to `127.0.0.1` only. The admin API requires the per-launch random token, delivered to the admin window over a capability-restricted IPC command and kept in memory only (never localStorage); check the Host header and Origin; the admin window loads the Host URL, so there is no CORS on the API, WebSocket or `/admin` (only the build `assets/` directory and the template directory send `Access-Control-Allow-Origin: *`; see `docs/architecture.md`). "Dev" means the `tauri/custom-protocol` feature is off; only `tauri build` turns it on for distributable builds, so never ship a binary built without it. Overlay routes are read-only and never get OAuth tokens or admin API access. Never render user templates or overlays directly in the admin window; previews go in an iframe with `sandbox="allow-scripts"` only and receive data via `postMessage`. `/admin` sends a strict CSP, and the admin window may only navigate to `/admin` paths. Escape all viewer input. Store tokens only in Windows Credential Manager, wrap them in `SecretString`, and never put them in logs, plain files or backups.
- Simulated events (`source=simulation`) never go into real business records (redemption records, check-in records).
- No silent data loss. Back up the database before running migrations.
- Licenses: the project is MIT. Never add a GPL, LGPL or AGPL dependency; new Rust dependencies must pass `cargo deny check`, and frontend dependencies must be checked with `pnpm licenses list`. Bundled assets need a redistributable license recorded in `assets/CREDITS.md`.
- The repo is public. Never commit secrets, tokens or personal data. Twitch auth uses the Device Code Flow, so there is no client secret; each user registers their own Twitch app and stores the Client ID in settings, so never hard-code one.

## Conventions
- Docs: `README.md` and everything under `docs/` (except `docs/archive/`) are written in Traditional Chinese (`zh-TW`), which is authoritative. Each has an English translation `<name>.en.md`. This file is English only.
- Write Chinese only while working. Before committing docs, create or update each `<name>.en.md`, set its `source-sha256` header, have the user review it, and run `node scripts/check-translations.mjs` (details in `docs/dev.md`).
- Code identifiers, comments, API/DB fields and error codes: English.
- Commits: English Conventional Commits, e.g. `feat(checkin): add daily check-in rule`.
- Branches: work on `feat/<slice>`, open a PR, squash-merge into `main` after CI passes.
- Backlog lives in GitHub Issues, not in docs.
- When behavior, layer rules or tech choices change, update the matching doc (see `docs/dev.md`).
