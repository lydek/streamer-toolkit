-- The break / starting-soon card: a single row with its settings and, while shown, when its countdown ends, so a
-- restart or an overlay reload shows the same time.
CREATE TABLE overlays_break_card (
    id INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
    text TEXT NOT NULL,
    end_text TEXT NOT NULL,
    -- 'minutes' or 'until'.
    mode TEXT NOT NULL,
    minutes INTEGER NOT NULL,
    -- Local time of day, HH:MM, for the 'until' mode.
    until_time TEXT NOT NULL,
    shown INTEGER NOT NULL,
    -- UTC, RFC 3339; set while shown.
    ends_at TEXT
);
