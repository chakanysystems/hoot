CREATE TABLE IF NOT EXISTS contacts (
    pubkey TEXT PRIMARY KEY NOT NULL,
    petname TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);
