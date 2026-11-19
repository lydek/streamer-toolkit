-- The message is the sender's first in the channel (Twitch's mark, or the toolkit's first sighting when that
-- cannot be read).
ALTER TABLE chat_messages ADD COLUMN first_chat INTEGER NOT NULL DEFAULT 0;
