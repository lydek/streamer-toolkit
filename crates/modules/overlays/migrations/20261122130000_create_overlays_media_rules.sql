-- Which video or GIF a channel points reward shows, where, and how loud. One row per reward.
CREATE TABLE overlays_media_rules (
    reward_id TEXT PRIMARY KEY NOT NULL,
    media_id TEXT NOT NULL,
    -- 'top_left', 'top_right', 'bottom_left', 'bottom_right', 'center' or 'full'.
    position TEXT NOT NULL,
    -- Percent; NULL follows the sound master volume.
    volume INTEGER
);
