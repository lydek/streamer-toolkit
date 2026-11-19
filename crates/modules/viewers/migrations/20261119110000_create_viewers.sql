-- One row per Twitch user seen in chat, alerts or redemptions; a new name for the same user ID updates the row.
CREATE TABLE viewers (
    twitch_user_id TEXT PRIMARY KEY NOT NULL,
    login TEXT NOT NULL,
    display_name TEXT NOT NULL,
    -- Twitch's image CDN only; NULL until looked up.
    avatar_url TEXT,
    -- The roles their chat badges showed last time they chatted.
    broadcaster INTEGER NOT NULL DEFAULT 0,
    moderator INTEGER NOT NULL DEFAULT 0,
    vip INTEGER NOT NULL DEFAULT 0,
    subscriber INTEGER NOT NULL DEFAULT 0,
    -- UTC, fixed-width RFC 3339 (microseconds), so text order is time order.
    first_seen_at TEXT NOT NULL,
    last_seen_at TEXT NOT NULL
);
