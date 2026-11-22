-- What each redemption showed (a check-in card, a reward sound, reward media) and how it ended, as the modules
-- that showed it report through the outbox (`presentation.redemption_played`). Records from before this existed
-- keep `playback_tracked = 0`, so they read as "no record" rather than "nothing played".
ALTER TABLE redemption_records ADD COLUMN playback_tracked INTEGER NOT NULL DEFAULT 0;

-- One row per outbox event: its ID keeps a redelivered event from adding a second row.
CREATE TABLE redemption_playback (
    event_id TEXT PRIMARY KEY NOT NULL,
    twitch_redemption_id TEXT NOT NULL,
    what TEXT NOT NULL CHECK (what IN ('card', 'sound', 'media')),
    result TEXT NOT NULL,
    -- UTC, fixed-width RFC 3339 (microseconds).
    recorded_at TEXT NOT NULL
);

CREATE INDEX redemption_playback_redemption ON redemption_playback (twitch_redemption_id);
