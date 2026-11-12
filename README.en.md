<!-- source: README.md source-sha256: d07e78ba15c9b4ee7b1722ee0ad11fa8cf56f35acefe7c67a38e9d595d59c06f -->
[繁體中文](README.md) | **English**

# Streamer Toolkit

A local-first Twitch streaming toolkit. It provides a customizable OBS chat overlay, event alerts, channel-point redemption records, daily check-ins and local sound effects, without relying on third-party hosted services.

**Principle: feature-first, fast iteration.** Build one feature slice at a time, then try it on a real stream.

**Status:** not implemented yet; the next step is a walking skeleton (see [scope](docs/scope.en.md)).

**Stack:** Rust + Axum, Tauri 2 + Vue 3 + TypeScript, SQLite + SQLx.

## How to run
To be added once the walking skeleton is done. For environment requirements, see [dev](docs/dev.en.md).

## Documents
Only these are maintained: the four documents below, complex-module specs `docs/modules/<module>.md`, and [AGENTS.md](AGENTS.md) for AI coding agents. Traditional Chinese is authoritative; English versions are the matching `.en.md` files.

- [docs/scope.en.md](docs/scope.en.md): what to build, when it counts as done, what not to build.
- [docs/architecture.en.md](docs/architecture.en.md): DDD + Clean Architecture layers, module rules, complex vs thin modules.
- [docs/decisions.en.md](docs/decisions.en.md): tech choices and when each is adopted.
- [docs/dev.en.md](docs/dev.en.md): environment, testing baseline, hard security constraints.

`docs/archive/` holds early planning drafts kept only on the local machine (not under version control), for reference only; they are not a spec. To-dos and ideas live in GitHub Issues. AI coding agents should read [AGENTS.md](AGENTS.md) first.

## License
MIT (see [LICENSE](LICENSE)).
