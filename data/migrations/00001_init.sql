CREATE TABLE IF NOT EXISTS events
(
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    kind       TEXT NOT NULL,
    payload    TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS updates
(
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    name       TEXT NOT NULL,
    version    TEXT,
    state      TEXT, -- pending, applied, failed, rolled_back
    meta       TEXT,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);