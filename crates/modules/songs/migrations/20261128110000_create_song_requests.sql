-- 點歌 (docs/modules/songs.md): requests and their states; the song's title and artist are copied, so the history
-- outlives the song.
CREATE TABLE songs_requests (
    -- UUID v7.
    id TEXT PRIMARY KEY NOT NULL,
    -- NULL once the song was deleted from the list.
    song_id TEXT REFERENCES songs_songs (id) ON DELETE SET NULL,
    title TEXT NOT NULL,
    artist TEXT NOT NULL DEFAULT '',
    -- Who it is for: Twitch user ID and display name (the name alone when typed by the streamer).
    viewer_id TEXT,
    viewer_name TEXT,
    -- 1: requested by the streamer on the page.
    by_hand INTEGER NOT NULL DEFAULT 0,
    state TEXT NOT NULL CHECK (state IN ('queued', 'singing', 'sung', 'removed')),
    -- Order among the queued requests, smallest first.
    position INTEGER NOT NULL,
    -- UTC, fixed-width RFC 3339 (microseconds).
    requested_at TEXT NOT NULL,
    -- When it was sung or removed.
    finished_at TEXT
);

-- At most one song is being sung.
CREATE UNIQUE INDEX songs_requests_one_singing ON songs_requests (state) WHERE state = 'singing';
-- A song is never queued or sung twice at once.
CREATE UNIQUE INDEX songs_requests_song_once ON songs_requests (song_id) WHERE state IN ('queued', 'singing');
CREATE INDEX songs_requests_sung ON songs_requests (state, finished_at);
