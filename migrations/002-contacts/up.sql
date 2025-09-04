CREATE TABLE IF NOT EXISTS profile_metadata (
    pubkey TEXT PRIMARY KEY,
    id TEXT NOT NULL,
    name TEXT,
    display_name TEXT,
    picture TEXT,
    created_at INTEGER NOT NULL
);
