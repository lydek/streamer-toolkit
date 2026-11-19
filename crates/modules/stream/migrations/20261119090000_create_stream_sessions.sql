-- One row per Twitch stream; the unique Twitch stream ID keeps a repeated start or check from opening a second.
CREATE TABLE stream_sessions (
    id TEXT PRIMARY KEY NOT NULL,
    twitch_stream_id TEXT NOT NULL UNIQUE,
    -- UTC, fixed-width RFC 3339 (microseconds), so text order is time order. Twitch's start time.
    started_at TEXT NOT NULL,
    -- NULL while live (or until the end is seen).
    ended_at TEXT
);

CREATE INDEX stream_sessions_started_at ON stream_sessions (started_at);
