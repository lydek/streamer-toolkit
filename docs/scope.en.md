<!-- source: docs/scope.md source-sha256: 1e9dcbb215fe6feedaaa04121c424666f3b8a1efd249db3234633cde0745bd80 -->
[繁體中文](scope.md) | **English**

# Scope

## Positioning
A local-first Twitch streaming toolkit that builds only what real streams need. The goal is to replace the third-party chat, alert and shoutout (SO) services currently in use.

## Responsibility boundaries
- Twitch owns channel-point balances, deductions and refunds. This tool only receives redemption events, stores records and triggers local actions.
- Local redemption records may be incomplete because of downtime or missed events. The tool never claims they match Twitch exactly.
- Twitch and OBS remain required external services. "Local-first" does not mean fully offline.
- Videos played in shoutouts still come from Twitch; if the source is gone, they cannot play.

## MVP feature slices
Build in order. Try each slice on a real stream when it is done, and move on only when it works. The order may change to fit streaming needs.

| # | Slice | What you can see when it is done |
|---|---|---|
| 0 | Walking skeleton | First-run setup guides registering a Twitch app and entering its Client ID, logs in with the Device Code Flow, stores tokens in keyring and refreshes them automatically; the Tauri window starts the local Host (if the port is taken, it says so and lets you change it); an OBS Browser Source loads a local URL and shows Twitch chat; every foundation marked "Build with the skeleton" in [decisions.en.md](decisions.en.md) is in place |
| 1 | Custom chat overlay | Editing the HTML/CSS template applies in OBS immediately; on disconnect the overlay shows the status and reconnects automatically; the admin UI shows Twitch and OBS (OBS WebSocket) connection status |
| 2 | Follow and subscription alerts | Events trigger overlay animations and sounds; testable with the event simulator, without waiting for a real follow |
| 3 | Redemption records | Every channel-point redemption is stored (deduplicated by redemption ID) and searchable in the UI; the first complex module |
| 4 | Daily check-in | Viewers check in by redeeming channel points or with a chat command; the streamer can manually register or backfill check-ins in the UI; one check-in per viewer per business day, with the day boundary set by the streamer's time zone; old check-in data can be imported (preview before import, deduplication, original file untouched; supported formats to be confirmed) |
| 5 | Sound management | Import local sound files and choose which event triggers each one |
| 6 | Backup and restore | One-click export/import of settings and records, with all data preserved after import |
| 7 | Windows installer | Installable and usable on another PC; the installer includes the third-party license list |

**After the MVP (confirmed needs):** personal badges, shoutout clips. Technical follow-ups are listed in [decisions.en.md](decisions.en.md).

**Other ideas:** go to the backlog; not scheduled, no specs.

## Out of scope for this round
Raffles, song requests.

## Not doing
General rules engine, Linux, native mobile apps, cloud services.

## Three questions before adding a feature
1. Does it solve a problem I have already hit while streaming?
2. What is the smallest version I can use on a stream?
3. Will it slow down the slice in progress?
