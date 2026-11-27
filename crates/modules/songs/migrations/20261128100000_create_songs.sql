-- 點歌 (docs/modules/songs.md): the song list and its categories.
CREATE TABLE songs_categories (
    -- UUID v7, so ID order is creation order.
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    -- The name normalised (case, width, blanks): two categories never share it.
    name_key TEXT NOT NULL UNIQUE,
    -- 1: songs in it can be requested.
    requestable INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE songs_songs (
    id TEXT PRIMARY KEY NOT NULL,
    title TEXT NOT NULL,
    artist TEXT NOT NULL DEFAULT '',
    -- Title and artist normalised: the same song is stored once.
    song_key TEXT NOT NULL UNIQUE,
    -- NULL: uncategorised. A deleted category leaves its songs uncategorised.
    category_id TEXT REFERENCES songs_categories (id) ON DELETE SET NULL,
    youtube_url TEXT,
    -- UTC, fixed-width RFC 3339 (microseconds); NULL: never sung.
    last_sung_at TEXT,
    request_count INTEGER NOT NULL DEFAULT 0
);
