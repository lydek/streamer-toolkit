-- Outbox events this module already applied, written in the same transaction as their effect.
CREATE TABLE checkin_inbox (
    event_id TEXT PRIMARY KEY NOT NULL,
    processed_at TEXT NOT NULL
);
