-- The sub marathon timer: a single row, written on every change and while live at each status (about once a
-- minute), so a restart knows where the clock stood and when the stream was last seen live.
CREATE TABLE counter_marathon (
    id INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
    -- As of anchor_at.
    remaining_ms INTEGER NOT NULL,
    -- UTC, RFC 3339.
    anchor_at TEXT NOT NULL,
    paused INTEGER NOT NULL,
    -- The stream session the clock has counted in.
    session TEXT,
    sub_minutes INTEGER NOT NULL,
    bits_minutes INTEGER NOT NULL,
    max_hours INTEGER NOT NULL
);
