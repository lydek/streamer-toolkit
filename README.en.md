<!-- source: README.md source-sha256: c6c9cf3e2365496bbe5814f2672927b030a4618ff270e2d4f315b45237ffbffd -->
[繁體中文](README.md) | **English**

# Streamer Toolkit

A local-first Twitch streaming toolkit. It provides a customizable OBS chat overlay, event alerts, channel-point redemption records, daily check-ins and local sound effects, without relying on third-party hosted services.

**Principle: feature-first, fast iteration.** Build one feature slice at a time, then try it on a real stream.

**Status:** the base features (chat, alerts, redemption records, check-in, sounds, backup and restore, portable build) are done; the remaining features are being added in phases toward the prototype, and there is no stable release yet (see [scope](docs/scope.en.md)).

**Stack:** Rust + Axum, Tauri 2 + Vue 3 + TypeScript, SQLite + SQLx.

## How to run
Once the first stable release is out, download the portable zip from GitHub Releases, extract it and run `Streamer Toolkit.exe`; the SmartScreen and "Unblock" steps are in the zip's `使用說明.txt`. To build from source and develop, see [dev](docs/dev.en.md).

## Documents
Only these are maintained: the four documents below, complex-module specs `docs/modules/<module>.md`, and [AGENTS.md](AGENTS.md) for AI coding agents. Traditional Chinese is authoritative; English versions are the matching `.en.md` files.

- [docs/scope.en.md](docs/scope.en.md): what to build, when it counts as done, what not to build.
- [docs/architecture.en.md](docs/architecture.en.md): DDD + Clean Architecture layers, module rules, complex vs thin modules.
- [docs/decisions.en.md](docs/decisions.en.md): tech choices and when each is adopted.
- [docs/dev.en.md](docs/dev.en.md): environment, testing baseline, hard security constraints.

`docs/archive/` holds early planning drafts kept only on the local machine (not under version control), for reference only; they are not a spec. To-dos and ideas live in GitHub Issues. AI coding agents should read [AGENTS.md](AGENTS.md) first.

## License
MIT (see [LICENSE](LICENSE)).
