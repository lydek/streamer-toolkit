-- Counts kept per viewer, e.g. who guessed the death count right.
CREATE TABLE counter_viewer_counters (
    -- UUID v7, so ID order is creation order.
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL
);

-- One row per counter and viewer (Twitch user ID); names come from the viewers module, never joined here.
CREATE TABLE counter_viewer_counts (
    counter_id TEXT NOT NULL REFERENCES counter_viewer_counters (id) ON DELETE CASCADE,
    twitch_user_id TEXT NOT NULL,
    value INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (counter_id, twitch_user_id)
);
