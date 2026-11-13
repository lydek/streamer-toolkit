<!-- source: docs/architecture.md source-sha256: b6fc4f13cf2bb3bc0664f945d94fd8f0ff9eeece4ce197fc563c382f753de637 -->
[繁體中文](architecture.md) | **English**

# Architecture

DDD + Clean Architecture, implemented as a Modular Monolith: one program, one crate per business module, layers inside each module. Adopted progressively: DDD tactical patterns are used only in complex modules. Tech choices and adoption timing are in [decisions.en.md](decisions.en.md).

## 1. Initial layout
Adjust during the skeleton as needed, then update this section.

```text
crates/
  core/            # Shared core: settings, module registry and lifecycle, error codes, event envelope, in-memory event broadcast, outbox (with its own migrations)
  integrations/    # Twitch (EventSub) and OBS (obws) adapters: turn external events into internal events; handle OAuth, reconnect, backoff, rate limits
  modules/
    chat/          # Chat display (thin)
    alerts/        # Follow/subscribe alerts (thin)
    redemption/    # Redemption records (complex)
    checkin/       # Daily check-in (complex): subscribes to RedemptionRecorded and chat command events
    sound/         # Sound management (thin)
  host/            # Composition root: wires modules, Axum HTTP/WebSocket routes, overlay static pages
apps/
  desktop/         # Tauri 2 shell: starts the Host in-process, system tray, single-instance lock; the admin window loads the Host URL
  web/             # Vue 3 + TS: admin UI, overlay pages, small port-change page
```

## 2. Layers inside a module
Every module crate has three layer folders, thin modules included, so an upgrade needs no file moves.

| Layer | Contents | May depend on |
|---|---|---|
| `domain/` | Entities and rules; complex modules use Aggregate / Value Object / Domain Event | Only shared types from `core`; **must not** use `sqlx` or `axum` |
| `application/` | Use cases and ports (Repository traits, public service traits) | `domain` |
| `infrastructure/` | `db/`: SQLx Repository implementations and Rows; `http/`: Axum handlers and DTOs (OpenAPI generated); event mapping | `application`, `domain` |

- A module's `lib.rs` exposes only application service traits, public DTOs and public events; everything else is `pub(crate)`.
- Complex modules: Domain, DTO, Row and public Event are separate types; `domain` has no `serde`/`sqlx` derives, and mapping happens at the boundary.
- Thin modules: `domain` holds only structs and simple validation, without Aggregate / Value Object; Rows may double as models, with only DTOs split out.

## 3. Complex and thin modules
A module is **complex**, and uses DDD tactical patterns, if any of these four questions is answered "yes":
1. Is there a rule that must always hold? (e.g. a redemption ID is stored only once; a viewer can check in only once per business day)
2. Are there state transitions, some of them invalid? (e.g. redemption statuses follow Twitch: from `UNFULFILLED` to `FULFILLED` or `CANCELED` (`CANCELED` means the points were refunded); a `CANCELED` one can never become valid again)
3. Would a bug damage data or viewer trust, not just the display?
4. Do other modules depend on these rules, or must the rules apply consistently in several places?

If all four are "no", the module is **thin**: settings CRUD, or turning events into visuals and sound. A thin module becomes complex as soon as its first such rule appears (fill in the domain model); there is no downgrade.

| Module | Type | Reason |
|---|---|---|
| Redemption records | Complex | Deduplication, `UNFULFILLED`/`FULFILLED`/`CANCELED` state transitions, marking records as possibly incomplete |
| Daily check-in | Complex | Once per day, business time zone, streaks, three trigger sources, import deduplication |
| Personal badges (after MVP) | Thin | First version is only "viewer/role → badge"; upgrade when rule priorities appear |
| Event alerts | Thin | Queueing, priority and replay are playback logic in the application layer |
| Chat display, sound management, shoutout clips, settings | Thin | CRUD or rendering, no data rules |

## 4. Between modules
- **Synchronous queries**: call the other module's public application service trait; never use its internal types.
- **Two kinds of events**:
  - Integration and UI events (chat messages, simulated events, connection status, etc.) are only broadcast in memory and never written to the database.
  - Module domain events (e.g. `RedemptionRecorded`) go through the outbox, and interested modules subscribe to them.
- **Outbox** (built in the redemption records slice):
  - The event and the business data are written in the same transaction.
  - The dispatcher takes events from the outbox, retries failures and shows them in the UI, and resends after a restart.
  - Delivery state is tracked per subscriber, so one failing subscriber neither re-runs nor blocks the others.
  - Subscribers must be safe to run more than once: processed event IDs go into the subscriber's own inbox table, in the same transaction as the business effect.
  - The outbox table belongs to `core`, which has its own migration folder; writing to the outbox reuses the caller's transaction.
  - The set of subscribers (disabled modules included) is recorded when an event is published; modules added later do not receive past events.
  - Subscribers derive business time (such as the check-in business day) from the event's `occurredAt`, not from processing time, so replayed events never land on the wrong day.
  - Retries are capped; past the cap, the event moves to a dead-letter queue shown in the UI with "retry" and "skip" actions, so one event cannot block its subscriber forever.
  - Events delivered to every subscriber are cleaned up periodically, together with pending deliveries for removed modules.
- **Disabling modules**:
  - From the skeleton on, modules have an enable/disable switch; while a module is disabled, in-memory events (chat, etc.) are dropped, not kept.
  - Once the outbox exists (redemption records slice), the streamer chooses when disabling:
    - "Pause": events stay in the outbox undelivered and are delivered in order after re-enabling; retries are not counted while paused; the UI shows the backlog size and the time of the oldest event.
    - "Off": events from the disabled period are marked skipped for that module, with no replay after re-enabling; before choosing, a warning lists the affected events and modules.
    - Switching from "pause" to "off" marks the undelivered events as skipped too.
- **Data**: one SQLite file; each module has its own tables (module prefix) and its own migration folder; **no cross-module joins**: use the public interface to get another module's data. Write transactions always use `BEGIN IMMEDIATE` (or a single writer connection) to avoid SQLite returning `SQLITE_BUSY`.
- **Migrations**: files use timestamp versions, and `host` merges them into a single Migrator at startup. Rules:
  - Versions must be globally unique and sorted by version after merging; a test checks this.
  - Released migration files must never change (SQLx compares checksums; even a line-ending change breaks startup).
  - Disabled modules must still provide their migrations, or startup fails on missing versions; even if a module is removed, its migration files are kept.
  - A dev command applies the merged migrations for SQLx offline data (`.sqlx/`).
- If a dependency cycle appears, move the shared event or type into `core` instead of letting two modules depend on each other.
- External platforms (Twitch, OBS) appear only in `integrations`; business modules never depend on external SDKs and only receive internal events. External values are converted at the integration layer; for example, EventSub redemption statuses are lowercase (`unfulfilled`/`fulfilled`/`canceled`, plus `unknown`) while the Helix API uses uppercase, so both are mapped to internal statuses and `unknown` is handled.

## 5. External communication
- The only MVP clients are the admin UI (Tauri window) and OBS overlays, both using HTTP + WebSocket (except the IPC commands listed under "Tauri IPC permissions" below), bound to `127.0.0.1` only. OBS Dock and mobile browser control come after the MVP.
- HTTP carries commands and queries; WebSocket pushes events. Every time a client connects to the WebSocket, the first message is a snapshot (the Host subscribes to events before taking the snapshot, so nothing is missed or repeated between them), and later events follow in increasing `seq`; a reconnect gets a fresh snapshot.
- WebSocket events use a fixed envelope from the first version: `id`, `type`, `version`, `seq`, `occurredAt`, `source`. Internal domain events are never exposed directly; `infrastructure` maps them to public events.
- API contract is code-first: OpenAPI and frontend TS types are generated from Rust, and CI checks that the generated files are up to date.
- **Port**: a fixed default port, `47810`. If it is taken, do not switch ports automatically, so OBS URLs never break silently. The Host is then not running and the settings page cannot open, so the small port-change page opens instead (served directly by Tauri; see "Frontend files" below): it shows the error, lets the user enter and validate a new port, saves it and retries the bind. Its strings also use resource keys.
- **Frontend files**:
  - `apps/web` has three kinds of entries: the admin UI, each overlay, and the small port-change page.
  - The admin UI and overlay build output is embedded into the executable with `rust-embed` and served by the Host.
  - The small port-change page builds into its own folder, which serves as Tauri's `frontendDist` (containing only this page).
  - Tauri sets `app.windows: []`, so the config defines no windows; all windows are created from Rust (in `setup`, or after a successful port retry).
  - "Dev" means the `tauri/custom-protocol` feature is off (`cfg(dev)` / `tauri::is_dev()`), not `debug_assertions`. `tauri build` (including `--debug`) turns it on and is the only distributable build; `tauri dev` and plain cargo commands without this feature (`cargo run`, `cargo build --release`, `cargo test`, `cargo clippy`) are dev. CI turns the feature on manually only for checking and distributes nothing.
  - `cfg(dev)` only applies inside `apps/desktop`: `apps/desktop` decides whether this is dev and passes the extra Vite origin allowed in dev to the Host as runtime configuration; `crates/host` never reads a cfg.
  - `rust-embed` enables the `debug-embed` feature, so debug builds also embed the files into the executable.
  - User-customized HTML/CSS templates live in the user data directory.
- **Dev mode**: the frontend runs on the Vite dev server with hot reload, proxying `/api` and `/ws` to the Host:
  - The proxy target is `http://127.0.0.1:<port>`, not `localhost` (Node may resolve that to IPv6 `::1`, while the Host binds IPv4 only).
  - The proxy sets `changeOrigin: true` (rewrites only the Host header, not Origin) and `ws: true`.
  - Tauri's `build.devUrl` is set to the Vite URL, so in development both the admin window and the small port-change page are served by Vite and count as a local origin; Vite uses `server.strictPort: true` so the port cannot drift away from the allowlist.
  - Vite sends the same CSP as `/admin` through `server.headers`, so violations show up during development.
  - In dev mode, Vite allows the `null` origin on every response except the proxied `/api` and `/ws` (`server.cors: false` plus a `configureServer` middleware), because the preview iframe loads modules from paths such as `/@vite/client` and `/src/...`.

### Local API security
Binding to `127.0.0.1` does not stop malicious web pages in the browser (cross-site requests, DNS rebinding, WebSocket hijacking), so the following is also required:
- **Admin window**: loads `http://127.0.0.1:<port>/admin` and is created only after the Host has bound the port; in development it loads the Vite dev server instead. The admin window and the API share one origin, so there is no CORS and no Tauri origin to handle. The admin window may only navigate to `/admin` paths on the app's own origin (`on_navigation` restriction), and opening new windows (`window.open`, `target=_blank`) is always denied (`on_new_window`); the Device Code Flow verification URL is opened in the system browser by a Rust command using `tauri-plugin-opener`, and the window shows only the code.
  - The Host sends a strict `Content-Security-Policy` on `/admin` (and every path under it): `script-src 'self'` (no inline scripts, no eval); `connect-src 'self' ipc: http://ipc.localhost` (Tauri IPC); `style-src 'self' 'unsafe-inline'` (PrimeVue injects styles at runtime, or use PrimeVue's nonce instead); `img-src 'self' data:` and `font-src 'self' data:` (Vite inlines small files as `data:`); `frame-src 'none'` (the release IPC capability covers the whole Host origin, so same-origin pages must not be framed inside the admin window; when template previews are added, allow only the sources the sandboxed iframe needs). No `default-src`. vue-i18n uses precompiled messages, so `unsafe-eval` is not needed.
- **Admin token**: the Host generates a random token at every launch; it is never written to disk. The admin window fetches it at startup through a Tauri IPC command; the token stays in memory only, never in localStorage (WebView2 writes that to disk). Admin HTTP requests carry an `Authorization` header; admin WebSocket connections send the token in their first message.
- **Tauri IPC permissions**: app commands are declared in `build.rs` with `tauri_build::AppManifest`, and every command needs an explicit grant. There are three capabilities:
  1. Small port-change page (local origin, `port` window only): grants only the commands that validate and save the port and retry the bind.
  2. Admin window, dev (local origin through `devUrl`, `admin` window only): grants admin commands such as fetching the token.
  3. Admin window, release build (the Host URL is a remote origin): created at startup with `Manager::add_capability`, with URLs limited to `http://127.0.0.1:<port>` and the `admin` window only (Tauri matches the IPC request's Origin, which has no path, so it cannot be limited to `/admin`; `on_navigation` restricts the path). It is not placed in the auto-enabled `capabilities/` folder; keep Tauri's `dynamic-acl` feature (on by default).
- **User templates**:
  - Custom templates and overlays are never rendered directly in the admin window; previews go in an iframe with only `sandbox="allow-scripts"` (no `allow-same-origin`), so template code cannot reach the admin window's IPC or token; a test checks that `invoke` from the preview iframe is rejected.
  - The preview iframe's origin is `null`, so it cannot connect to the Host; the admin window passes snapshots and sample events in with `postMessage`.
  - Every response on overlay and template paths (HTML and SVG included) sends the overlay CSP: `script-src 'self'; object-src 'none'`; the template directory serves only non-script file types (allowlist: HTML, CSS, images, fonts, sounds) with `X-Content-Type-Options: nosniff`, enforcing HTML/CSS-only templates in the first version.
- **Host header**: accept only `127.0.0.1:<port>` and `localhost:<port>`, to block DNS rebinding.
- **Origin**: every WebSocket connection (overlays included) and every state-changing request checks Origin, accepting only `http://127.0.0.1:<port>` and `http://localhost:<port>` (current port); dev mode also accepts the Vite dev server origin (passed in by `apps/desktop` as runtime configuration). WebSocket connections without an Origin or with `Origin: null` are always rejected.
- **CORS**: none on `/api`, `/ws` and `/admin`. Only the build `assets/` directory (chunks shared by the admin UI and overlays) and the template directory send `Access-Control-Allow-Origin: *` (without credentials; the content is public code and user templates), because the preview iframe's origin is `null` and Vite's module scripts, CSS and fonts must load through CORS.
- **Overlays**: routes are read-only, can only read snapshots and subscribe to public events, and have no state-changing endpoints; OBS URLs contain no token.
- **OBS Dock**: not in the MVP; its pairing will be designed later together with mobile remote control.

### Twitch rate limits
Twitch's official documentation is the source for these numbers; update this section when Twitch changes them.
- **Helix API**: a token bucket per client ID and user, refilled every minute (for example 800 points; most endpoints cost 1 point per call). Every Helix request goes through the one Helix client in `integrations`: a 401 gets one refresh and one retry; a 429 waits until `Ratelimit-Reset` (at most 1 minute) before trying again and is never resent right away. Some 429s are unrelated to the points bucket (for example EventSub limits); without `Ratelimit-Reset`, normal backoff applies.
- **EventSub WebSocket**: per user token (client ID and user), at most 3 connections with enabled subscriptions, at most 300 subscriptions per connection, and a `max_total_cost` of 10. The app opens only 1 connection per account and closes the old one before changing the Client ID or signing out; on `session_reconnect` it moves to the reconnect URL Twitch sends (this is not a new connection; subscriptions carry over and are not created again); failed connections reconnect with exponential backoff (from 1 second, at most 60 seconds).
- **Device Code Flow**: poll at the `interval` Twitch returns until `expires_in`, never faster, and at least 1 second apart.
- **Sending chat messages**: the MVP sends no chat messages. When sending is added later (for example check-in replies), messages must be queued and rate-limited locally, using the conservative value of 20 messages per 30 seconds. Twitch allows 20 messages per 30 seconds for regular accounts and 100 per 30 seconds for the broadcaster, moderators and VIPs (messages from the broadcaster still count toward the 20-message bucket); going over makes Twitch ignore the account's messages for 1 hour.

## 6. Host lifecycle
The Host runs in the Tauri process. Closing the admin window only hides it to the system tray and overlays keep running; only quitting from the tray stops the Host.

Startup order:
1. Register `tauri-plugin-single-instance` first and confirm this is the only instance, so a second instance does not try to take the port and fail before handing off.
2. The Host binds the port; on failure, open the small port-change page and retry after the user saves.
3. Release build: create the remote capability for the current port, then create the admin window and load the Host URL. Dev mode: the admin window loads the Vite dev server and uses capability 2.

## 7. Enforcing the rules
- Cross-module dependencies: enforced by each crate's `Cargo.toml`.
- Layer rules: architecture tests run in CI and at least check that `domain/` contains no occurrence of `sqlx`/`axum` at all (including `use`, derives and full paths) and that modules do not use other modules' non-public types.
