-- On-stream warnings: which are on, how long the mic may stay muted, and the OBS input to watch. A single row.
CREATE TABLE overlays_warnings (
    id INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
    chat_overlay INTEGER NOT NULL,
    twitch INTEGER NOT NULL,
    mic INTEGER NOT NULL,
    mic_seconds INTEGER NOT NULL,
    -- NULL: the mic is not checked.
    mic_input TEXT
);
