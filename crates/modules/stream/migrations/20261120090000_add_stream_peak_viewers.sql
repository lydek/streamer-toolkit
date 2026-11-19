-- The most viewers any check counted during the stream; NULL until one has.
ALTER TABLE stream_sessions ADD COLUMN peak_viewers INTEGER;
