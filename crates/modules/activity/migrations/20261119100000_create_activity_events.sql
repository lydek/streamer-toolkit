-- One row per Twitch follow, sub, resub, gift (and each sub it hands out), cheer or raid. The unique EventSub
-- message ID keeps a redelivered notification from adding a second row.
CREATE TABLE activity_events (
    id TEXT PRIMARY KEY NOT NULL,
    twitch_message_id TEXT NOT NULL UNIQUE,
    kind TEXT NOT NULL CHECK (kind IN ('follow', 'subscribe', 'resubscribe', 'gift', 'cheer', 'raid')),
    -- The viewer, or for a raid the raiding channel; NULL for an anonymous gift or cheer.
    user_id TEXT,
    user_login TEXT,
    user_name TEXT,
    tier INTEGER,
    months INTEGER,
    count INTEGER,
    bits INTEGER,
    viewers INTEGER,
    -- A sub handed out by a gift.
    gifted INTEGER NOT NULL DEFAULT 0,
    message TEXT,
    -- The stream session it happened in (the stream module's ID; no join across modules).
    session_id TEXT,
    -- UTC, fixed-width RFC 3339 (microseconds), so text order is time order.
    occurred_at TEXT NOT NULL
);

CREATE INDEX activity_events_occurred_at ON activity_events (occurred_at);
