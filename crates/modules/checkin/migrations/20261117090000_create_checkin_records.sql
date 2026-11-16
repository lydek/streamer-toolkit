-- Viewers who checked in. Internal IDs are UUID v7; Twitch's user ID is the external key.
CREATE TABLE checkin_viewers (
    id TEXT PRIMARY KEY NOT NULL,
    twitch_user_id TEXT NOT NULL UNIQUE,
    user_login TEXT NOT NULL,
    user_name TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

-- One row per check-in. The unique pair keeps a viewer to one check-in per business day on its own.
CREATE TABLE checkin_records (
    id TEXT PRIMARY KEY NOT NULL,
    viewer_id TEXT NOT NULL REFERENCES checkin_viewers (id),
    -- YYYY-MM-DD, judged in the broadcaster's time zone and reset hour in force at the time.
    business_day TEXT NOT NULL,
    -- UTC, fixed-width RFC 3339 (microseconds), so text order is time order.
    occurred_at TEXT NOT NULL,
    trigger_kind TEXT NOT NULL CHECK (trigger_kind IN ('chat', 'redemption', 'manual')),
    recorded_at TEXT NOT NULL,
    UNIQUE (viewer_id, business_day)
);

CREATE INDEX checkin_records_viewer_time ON checkin_records (viewer_id, occurred_at);
CREATE INDEX checkin_records_day ON checkin_records (business_day);
