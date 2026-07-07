CREATE TABLE IF NOT EXISTS deleted_events (
    event_id TEXT PRIMARY KEY,
    author_pubkey TEXT,
    deleted_at INTEGER NOT NULL DEFAULT (unixepoch()),
    source_event_id TEXT
);

CREATE INDEX idx_deleted_events_author ON deleted_events (author_pubkey);

CREATE TABLE IF NOT EXISTS gift_wrap_map (
    wrap_id TEXT PRIMARY KEY,
    inner_id TEXT NOT NULL,
    recipient_pubkey TEXT,
    created_at INTEGER
);

CREATE INDEX idx_gift_wrap_inner_id ON gift_wrap_map (inner_id);
