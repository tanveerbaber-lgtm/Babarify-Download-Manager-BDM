-- 0001: initial schema — download records and key/value settings.

CREATE TABLE downloads (
    id               TEXT    PRIMARY KEY NOT NULL,
    url              TEXT    NOT NULL,
    file_name        TEXT    NOT NULL,
    destination_dir  TEXT    NOT NULL,
    status           TEXT    NOT NULL DEFAULT 'pending'
                     CHECK (status IN ('pending', 'queued', 'downloading', 'paused',
                                       'completed', 'failed', 'cancelled')),
    total_bytes      INTEGER CHECK (total_bytes IS NULL OR total_bytes >= 0),
    downloaded_bytes INTEGER NOT NULL DEFAULT 0 CHECK (downloaded_bytes >= 0),
    error_message    TEXT,
    created_at       INTEGER NOT NULL, -- unix epoch milliseconds
    updated_at       INTEGER NOT NULL,
    completed_at     INTEGER
);

CREATE INDEX idx_downloads_status     ON downloads (status);
CREATE INDEX idx_downloads_created_at ON downloads (created_at DESC);

CREATE TABLE settings (
    key        TEXT    PRIMARY KEY NOT NULL,
    value      TEXT    NOT NULL, -- JSON-encoded value
    updated_at INTEGER NOT NULL
);
