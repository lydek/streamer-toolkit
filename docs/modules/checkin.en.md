<!-- source: docs/modules/checkin.md source-sha256: 421a8fc7e86361f97acb15dbe0b66337d3c0b08213121609100922926d17b204 -->
[繁體中文](checkin.md) | **English**

# Daily check-in

A complex module (see [architecture.en.md](../architecture.en.md) §3). This document covers only rules and acceptance.

## Rules
- A viewer checks in by typing `!<command>` in chat: only the first word of the message counts, and ASCII letters ignore case. The command is a setting (default `打卡`), stored without the `!`.
- A channel points reward can also be chosen in the settings (its Twitch reward ID is stored; the list is read from Twitch): redeeming it checks the viewer in. The check-in module receives redemption records' `RedemptionRecorded` through the outbox; with no reward set, or for any other reward, nothing is checked in. A refunded redemption does not undo the check-in.
- One redemption checks in only once: handled event IDs are kept in an inbox, written in the same transaction as the check-in, so redelivery changes nothing.
- A viewer counts once per business day, whether the command or the redemption came first. A unique constraint in the database (viewer + business day) also blocks repeats, not just the code.
- Viewers are identified by their Twitch user ID (renaming does not matter); internal IDs are UUID v7, with the Twitch user ID stored in its own column. The name kept is the latest one seen.
- Simulated events (`source=simulation`) are never written to check-in records.
- Redemptions made while the program is closed never arrive (a Twitch limitation); redemptions received while it runs but not yet handled are applied after a restart, counted for the business day of the redemption.
- While the module is disabled, chat check-in commands are dropped, like other in-memory events; existing records are kept.
- Disabling offers "pause" or "close" ([architecture.en.md](../architecture.en.md) §4):
  - Pause: redemptions stay in the outbox and retries are not counted; the settings page shows how many are waiting and when the oldest was redeemed; after re-enabling they check in in order, on the business day of each redemption.
  - Close: a warning comes first; redemptions from the disabled period (including those kept while paused) are marked skipped and never applied after re-enabling. Switching from pause to close skips the kept ones too.
- A redemption that keeps failing (5 attempts) moves to the failed list and no longer holds back later ones; the settings page lists it with "retry" (attempts start over) and "skip".
- Times are stored in UTC; when an event's time cannot be read, the time it was received is used.

## Admin UI
- The home "check-in" tile and drawer show how many viewers checked in today (the current business day) and the latest few; the full page lists each viewer's total, stamp card and last check-in day, searchable by login or display name.
- The broadcaster can register a known viewer for today or back-fill a past business day (recorded at the start of that day); future business days are refused. Manual check-ins and back-fills follow the same once-per-business-day rule, and a repeat returns "already checked in" with a notice, not an error.
- Viewer names are shown as text only. Deleting or editing single records, removing stamps and exporting are not offered.

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
- One redemption checks in only once (redelivery included); a chat check-in followed by a redemption on the same day counts once; a redemption delivered late counts for the business day it was redeemed.
- After pausing and re-enabling, redemptions from that period are applied in order, on the day each was redeemed; after closing and re-enabling, they are not applied.
- Events past the retry limit move to the failed list and do not hold back later events.
- Manual check-ins and back-fills follow the same once-per-business-day rule; a repeat shows a notice and is not counted again. After the number of card slots changes, every viewer's card is recalculated at once.
- Simulated events are never written to check-in records; nothing is recorded while the module is disabled.
- All of the above are tested (the repository against a real SQLite database).
