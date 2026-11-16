-- Domain events between modules (docs/architecture.md §4), written in the publisher's own transaction.
CREATE TABLE outbox_events (
    id TEXT PRIMARY KEY NOT NULL,
    event_type TEXT NOT NULL,
    payload TEXT NOT NULL,
    -- UTC, fixed-width RFC 3339 (microseconds), so text order is time order.
    occurred_at TEXT NOT NULL,
    published_at TEXT NOT NULL
);

-- Delivery state per subscriber, fixed when the event is published. `dead` waits for the broadcaster to retry or
-- skip it; `skipped` is never delivered.
CREATE TABLE outbox_deliveries (
    event_id TEXT NOT NULL REFERENCES outbox_events (id) ON DELETE CASCADE,
    subscriber TEXT NOT NULL,
    state TEXT NOT NULL CHECK (state IN ('pending', 'delivered', 'dead', 'skipped')),
    attempts INTEGER NOT NULL DEFAULT 0,
    last_error TEXT,
    next_attempt_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY (event_id, subscriber)
);

CREATE INDEX outbox_deliveries_state ON outbox_deliveries (subscriber, state);
