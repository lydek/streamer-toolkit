<!-- source: docs/scope.md source-sha256: 0ba3795119e73c47833bcd664da94ad6728507e1d810998e49eab41884b8b05f -->
[繁體中文](scope.md) | **English**

# Scope

## Positioning
A local-first Twitch streaming toolkit that builds only what real streams need. The goal is to replace the third-party chat, alert and shoutout (SO) services currently in use.

## Responsibility boundaries
- Twitch owns channel-point balances, deductions and refunds. This tool only receives redemption events, stores records and triggers local actions.
- Local redemption records may be incomplete because of downtime or missed events. The tool never claims they match Twitch exactly.
- Twitch and OBS remain required external services. "Local-first" does not mean fully offline.
- Videos played in shoutouts still come from Twitch; if the source is gone, they cannot play.

## Goal
The target is the prototype's full feature set, in six groups: home, song requests, chat, interaction, screen and sound, and viewers. Work goes through the phases below; after each phase, try it on a real stream and publish a release.

## Base already built
All of these are done; the phases below build on them.

| # | Slice | What you can see when it is done |
|---|---|---|
| 0 | Walking skeleton | First-run setup guides registering a Twitch app and entering its Client ID, logs in with the Device Code Flow, stores tokens in keyring and refreshes them automatically; the Tauri window starts the local Host (if the port is taken, it says so and lets you change it); an OBS Browser Source loads a local URL and shows Twitch chat; every foundation marked "Build with the skeleton" in [decisions.en.md](decisions.en.md) is in place |
| 1 | Custom chat overlay | Picking a built-in template applies in OBS immediately; on disconnect the overlay shows the status and reconnects automatically; the admin UI shows Twitch and OBS (OBS WebSocket) connection status |
| 2 | Follows, subscriptions, Bits and raids | Follows, new subs, resubs, gift subs, Bits and raids trigger overlay animations and sounds; several at once play one after another, and one gift of subs plays only once; pick from built-in templates; testable with the event simulator, without waiting for a real follow |
| 3 | Redemption records | Every channel-point redemption received is stored (deduplicated by redemption ID) and searchable in the UI; the first complex module |
| 4 | Daily check-in | Viewers check in by redeeming channel points or with a chat command; the streamer can manually register or backfill check-ins in the UI; one check-in per viewer per business day, with the day boundary set by the streamer's time zone; a stamp card shows progress from the total check-ins; old check-in data can be imported (preview before import, deduplication, original file untouched; JSON format, see [modules/checkin.en.md](modules/checkin.en.md)) |
| 5 | Sound management | Import local sound files (MP3, WAV, OGG) and choose what alerts, check-ins and each channel point reward play; all sound plays in OBS, with reward sounds played in order on a separate OBS URL |
| 6 | Backup and restore | One-click export/import of settings and records, with all data preserved after import |
| 7 | Windows portable package | Usable on another PC after extracting the zip; the zip includes a list of third-party licenses and instructions for handling SmartScreen warnings and unblocking the zip. When a new version is available, the admin UI notifies the user; extracting the new zip over the existing folder updates the app without affecting existing data |

## Phases
Each phase depends only on earlier phases; the order within a phase follows streaming needs. Details are in GitHub Issues (labels `phase-0` to `phase-9`); technical follow-ups are listed in [decisions.en.md](decisions.en.md).

| # | Contents |
|---|---|
| 0 | Every screen in place: group tabs across the top, every page (unbuilt ones show "not available yet"), the feature on/off page, the persistent chat panel, customizable tabs, text size |
| 1 | Shared foundations: extended Twitch event fields, OBS control and events, the presentation queue, stream status and sessions, the activity log, viewer identity, overlay connection tracking |
| 2 | Features that only need the shared foundations: scenes and background music, home statistics, the event feed, chat history and display, counters, screen tools, the card editor, rehearsal and more |
| 3 | Requesting permissions per feature, and features that write to Twitch: chat bot, channel point reward management, stream info, polls and predictions and more |
| 4 | Song requests (optionally linked to Leihu Singsing) |
| 5 | Shoutouts and videos, credits, raids |
| 6 | Modes |
| 7 | Advanced viewer features, personal badges, chat moderation, chat filtering |
| 8 | Avatars (VTuber Plus, VSeeFace), live captions, text-to-speech |
| 9 | Hotkeys, always-on-top chat window, profiles, bot account, themes and languages |

**Other ideas:** go to the backlog; not scheduled, no specs.

## Not doing
General rules engine, Linux, native mobile apps, cloud services, raffles, OneComme integration.

## Three questions before adding a feature
1. Does it solve a problem I have already hit while streaming?
2. What is the smallest version I can use on a stream?
3. Will it slow down the phase in progress?
