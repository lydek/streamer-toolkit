<!-- source: docs/modules/checkin.md source-sha256: 78a98e0498f26fd39a36e6d79dcdcd4ec9bff9c1b74aff1a20b184cb6511358f -->
[繁體中文](checkin.md) | **English**

# Daily check-in

A complex module (see [architecture.en.md](../architecture.en.md) §3). This document covers only rules and acceptance.

## Rules
- A viewer checks in by typing `!<command>` in chat: only the first word of the message counts, and ASCII letters ignore case. The command is a setting (default `打卡`), stored without the `!`.
- A viewer counts once per business day. A unique constraint in the database (viewer + business day) also blocks repeats, not just the code.
- Viewers are identified by their Twitch user ID (renaming does not matter); internal IDs are UUID v7, with the Twitch user ID stored in its own column. The name kept is the latest one seen.
- Simulated events (`source=simulation`) are never written to check-in records.
- While the module is disabled, chat check-in commands are dropped, like other in-memory events; existing records are kept.
- Times are stored in UTC; when an event's time cannot be read, the time it was received is used.

## Business day
- Settings: time zone (IANA, default `Asia/Taipei`) and reset hour (local 0–23, default 0).
- The business day comes from the time the event happened (`occurredAt`), not from processing time: the date of the local time minus the reset hour. Example: with a 6 o'clock reset, 5 a.m. on 11/17 still counts as 11/16.
- Daylight saving time: when the reset hour does not exist locally (the skipped hour), the day starts when the clock reaches that time again; in a repeated hour, the first occurrence counts.
- Changing the time zone or reset hour: a check-in is a repeat when the viewer already has a record for its business day, or a record inside that business day's time range under the new settings. So a settings change never counts the same day twice; it may count one less (when an old record's business-day label equals a different day under the new settings).
- Values in the settings file that cannot be used fall back to their defaults one by one; the admin API rejects invalid values (`checkin.invalid_settings`).

## Stamp card
- Not stored; derived from the total: 0 stamps for a total of 0, otherwise `(total - 1) % slots + 1`. A just-completed card shows every slot; the next check-in starts a new card.
- The number of slots is a setting (1–100, default 10); after it changes, every viewer's card is recalculated at once.

## Acceptance
- Repeated check-ins on the same business day are not counted again; the business day follows the configured time zone and reset hour, judged from the time the event happened.
- After the time zone or reset hour changes, a viewer who already checked in that day is not counted again.
- Simulated events are never written to check-in records; nothing is recorded while the module is disabled.
- All of the above are tested (the repository against a real SQLite database).
