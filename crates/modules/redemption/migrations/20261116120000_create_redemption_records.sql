-- One row per Twitch redemption; the unique Twitch ID keeps a redelivered event from adding a second row.
CREATE TABLE redemption_records (
    id TEXT PRIMARY KEY NOT NULL,
    twitch_redemption_id TEXT NOT NULL UNIQUE,
    user_id TEXT NOT NULL,
    user_login TEXT NOT NULL,
    user_name TEXT NOT NULL,
    reward_id TEXT NOT NULL,
    reward_title TEXT NOT NULL,
    reward_cost INTEGER NOT NULL,
    user_input TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('unfulfilled', 'fulfilled', 'canceled', 'unknown')),
    -- UTC, fixed-width RFC 3339 (microseconds), so text order is time order.
    redeemed_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX redemption_records_redeemed_at ON redemption_records (redeemed_at);
