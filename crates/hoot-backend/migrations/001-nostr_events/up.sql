CREATE TABLE IF NOT EXISTS events (
    id TEXT PRIMARY KEY,
    pubkey TEXT NOT NULL GENERATED ALWAYS AS (jsonb_extract (raw, '$.pubkey')),
    created_at INTEGER NOT NULL GENERATED ALWAYS AS (jsonb_extract (raw, '$.created_at')) VIRTUAL,
    kind INTEGER NOT NULL GENERATED ALWAYS AS (jsonb_extract (raw, '$.kind')) VIRTUAL,
    tags BLOB NOT NULL GENERATED ALWAYS AS (jsonb_extract (raw, '$.tags')) VIRTUAL,
    content TEXT NOT NULL GENERATED ALWAYS AS (jsonb_extract (raw, '$.content')) VIRTUAL,
    sig TEXT GENERATED ALWAYS AS (jsonb_extract (raw, '$.sig')) VIRTUAL,
    raw BLOB NOT NULL
);

-- indexes
CREATE INDEX idx_events_pubkey ON events (pubkey);
CREATE INDEX idx_events_kind ON events (created_at);
