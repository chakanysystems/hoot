-- Add nip05 column to profile_metadata table
ALTER TABLE profile_metadata ADD COLUMN nip05 TEXT;

-- Create NIP-05 cache table
CREATE TABLE IF NOT EXISTS nip05_cache (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    pubkey TEXT NOT NULL,
    nip05 TEXT NOT NULL,
    is_own BOOLEAN NOT NULL DEFAULT 0,
    first_seen INTEGER NOT NULL DEFAULT (unixepoch()),
    last_verified INTEGER,
    last_checked INTEGER,
    UNIQUE(pubkey, nip05)
);

CREATE INDEX idx_nip05_pubkey ON nip05_cache (pubkey);
CREATE INDEX idx_nip05_own ON nip05_cache (is_own);
