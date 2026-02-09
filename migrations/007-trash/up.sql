CREATE TABLE IF NOT EXISTS trash_events (
    event_id TEXT PRIMARY KEY,
    trashed_at INTEGER NOT NULL DEFAULT (unixepoch()),
    purge_after INTEGER NOT NULL
);

CREATE INDEX idx_trash_events_purge_after ON trash_events (purge_after);
