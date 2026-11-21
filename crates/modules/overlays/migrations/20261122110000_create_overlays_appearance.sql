-- The built-in overlays' look: theme and text size. A single row.
CREATE TABLE overlays_appearance (
    id INTEGER PRIMARY KEY NOT NULL CHECK (id = 1),
    -- 'dark', 'cream' or 'contrast'.
    theme TEXT NOT NULL,
    -- Percent.
    text_size INTEGER NOT NULL
);
