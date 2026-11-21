-- The media library: GIFs and videos for redemptions. Each file is media/<id>.<format> in the data folder.
CREATE TABLE overlays_media (
    -- UUID v7, so ID order is import order.
    id TEXT PRIMARY KEY NOT NULL,
    title TEXT NOT NULL,
    -- 'gif', 'webm' or 'mp4'.
    format TEXT NOT NULL,
    -- NULL until a video's length has been read.
    duration_ms INTEGER
);
