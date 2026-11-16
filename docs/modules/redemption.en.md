<!-- source: docs/modules/redemption.md source-sha256: b801e9eabcfcae1695619d4bcc2099816821d26932d695bb8d1389abaac780b1 -->
[繁體中文](redemption.md) | **English**

# Redemption records

A complex module (see [architecture.en.md](../architecture.en.md) §3). This document covers only rules, status transitions and acceptance.

## Rules
- When a viewer redeems a custom channel points reward, one record is stored: viewer, reward name, points, the text the viewer typed, redemption time and status.
- One record per Twitch redemption ID. A unique constraint in the database also blocks duplicates, not only the code.
- The internal ID is a UUID v7; the Twitch redemption ID is kept in its own column.
- The status follows Twitch (EventSub `channel.channel_points_custom_reward_redemption.add` and `.update`).
- When a status update arrives before the redemption itself, the update carries the whole redemption and creates the record; the redemption event that arrives later counts as a duplicate and changes nothing.
- A redemption event received again changes nothing.
- Simulated events (`source=simulation`) are never stored.
- Redemptions received while the module is disabled are dropped, like other in-memory events; stored records are kept.
- When a record is first stored (whether the redemption event or a status update created it), `RedemptionRecorded` is published through the outbox in the same transaction, with `occurredAt` set to the redemption time; repeated events and status updates publish nothing more.
- The viewer's text is stored as-is and escaped when shown.
- Times are stored in UTC; when Twitch's redemption time cannot be read, the time received is used.

## Status transitions
| From | Can become |
|---|---|
| Unfulfilled (`unfulfilled`) | Fulfilled (`fulfilled`), refunded (`canceled`) |
| Unknown (`unknown`) | Unfulfilled, fulfilled, refunded |
| Fulfilled | Nothing else |
| Refunded | Nothing else; never valid again |

- Receiving the same status again is not a transition; the record stays the same.
- An invalid transition is rejected, the record stays as it was, and it is logged as `redemption.invalid_transition` (with the Twitch redemption ID and both statuses, never the viewer's text).
- A status value Twitch sends that is not recognized is stored as `unknown` without an error; a known status received later updates it per the table above. A known status is never overwritten by `unknown`.

## Acceptance
- The same redemption ID received again leaves one record.
- Statuses change only as in the table; refunded never becomes valid again; invalid transitions are rejected and logged.
- An `unknown` status is stored without an error.
- When a status update arrives before the redemption itself, no data is lost and the status does not go back.
- Simulated events create no records; nor does anything while the module is disabled.
- Browsing: filter by redemption time range, viewer (part of the login or display name), part of the reward name and status, newest first, in pages; an empty state when there are no records; the viewer's text is escaped when shown.
- All of the above are tested (the repository with real SQLite).
