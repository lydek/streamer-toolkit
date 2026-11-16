-- Imported check-in history: the total and last check-in day a viewer had before this toolkit. A later import
-- overwrites them, so importing the same file twice changes nothing; displayed totals add the toolkit's records.
ALTER TABLE checkin_viewers ADD COLUMN baseline_total INTEGER NOT NULL DEFAULT 0;
ALTER TABLE checkin_viewers ADD COLUMN baseline_last_day TEXT;
