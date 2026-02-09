CREATE TABLE IF NOT EXISTS drafts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    subject TEXT NOT NULL DEFAULT '',
    to_field TEXT NOT NULL DEFAULT '',
    content TEXT NOT NULL DEFAULT '',
    parent_events TEXT NOT NULL DEFAULT '[]',
    selected_account TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);
