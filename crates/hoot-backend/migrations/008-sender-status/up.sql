CREATE TABLE IF NOT EXISTS sender_status (
    pubkey TEXT PRIMARY KEY NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('allowed', 'junked')),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX idx_sender_status_status ON sender_status (status);
