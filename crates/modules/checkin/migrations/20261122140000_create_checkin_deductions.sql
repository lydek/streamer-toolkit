-- Stamps spent: each deduction lowers what a viewer can spend; the lifetime total and the card never change.
CREATE TABLE checkin_deductions (
    id TEXT PRIMARY KEY NOT NULL,
    viewer_id TEXT NOT NULL REFERENCES checkin_viewers (id),
    amount INTEGER NOT NULL CHECK (amount > 0),
    -- Optional; empty when none was given.
    reason TEXT NOT NULL,
    -- UTC, fixed-width RFC 3339 (microseconds), so text order is time order.
    deducted_at TEXT NOT NULL
);

CREATE INDEX checkin_deductions_viewer ON checkin_deductions (viewer_id);
CREATE INDEX checkin_deductions_time ON checkin_deductions (deducted_at);
