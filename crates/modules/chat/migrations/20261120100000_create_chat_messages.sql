-- One row per chat message from Twitch (simulated ones are never stored); the unique Twitch message ID keeps a
-- redelivered message from adding a second row.
CREATE TABLE chat_messages (
    id TEXT PRIMARY KEY NOT NULL,
    twitch_message_id TEXT NOT NULL UNIQUE,
    -- The stream session it arrived in (the stream module's ID; no join across modules); NULL before any stream.
    session_id TEXT,
    user_id TEXT NOT NULL,
    -- The `chat.message` payload as JSON; its text and fragments are emptied when a moderator deletes it.
    payload TEXT NOT NULL,
    -- A moderator deleted it (or cleared its sender or the whole chat).
    deleted INTEGER NOT NULL DEFAULT 0,
    -- UTC, fixed-width RFC 3339 (microseconds), so text order is time order.
    sent_at TEXT NOT NULL
);

CREATE INDEX chat_messages_sent_at ON chat_messages (sent_at);
CREATE INDEX chat_messages_session ON chat_messages (session_id, sent_at);
CREATE INDEX chat_messages_user ON chat_messages (user_id, sent_at);
