-- The channel the message was sent to (the broadcaster's Twitch user ID), so clearing one channel's chat never
-- reaches another account's messages; NULL for rows stored before it was kept, which no clear reaches.
ALTER TABLE chat_messages ADD COLUMN broadcaster_id TEXT;
