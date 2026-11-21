-- Global counters; their chat commands live in the shared command table (`chat_commands` in settings.json).
CREATE TABLE counter_counters (
    -- UUID v7, so ID order is creation order.
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    value INTEGER NOT NULL DEFAULT 0,
    -- 1: back to 0 when the next stream starts.
    per_stream INTEGER NOT NULL DEFAULT 0,
    -- The stream session it was last reset for (or that was current when it became per-stream); NULL: none yet.
    reset_session TEXT
);
