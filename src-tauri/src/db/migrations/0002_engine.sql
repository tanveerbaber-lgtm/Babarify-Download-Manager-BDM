-- 0002: download engine — remote identity, temp files, checksums and segments.

ALTER TABLE downloads ADD COLUMN final_url       TEXT;    -- URL after redirects
ALTER TABLE downloads ADD COLUMN temp_path       TEXT;    -- partial file while incomplete
ALTER TABLE downloads ADD COLUMN saved_path      TEXT;    -- final file once completed
ALTER TABLE downloads ADD COLUMN etag            TEXT;    -- remote identity for safe resume
ALTER TABLE downloads ADD COLUMN last_modified   TEXT;
ALTER TABLE downloads ADD COLUMN resumable       INTEGER NOT NULL DEFAULT 0; -- server honours Range
ALTER TABLE downloads ADD COLUMN expected_sha256 TEXT;    -- optional, lowercase hex

-- Byte ranges of a segmented download; end_byte is inclusive.
CREATE TABLE download_segments (
    download_id TEXT    NOT NULL REFERENCES downloads (id) ON DELETE CASCADE,
    idx         INTEGER NOT NULL,
    start_byte  INTEGER NOT NULL CHECK (start_byte >= 0),
    end_byte    INTEGER NOT NULL CHECK (end_byte >= start_byte),
    downloaded  INTEGER NOT NULL DEFAULT 0 CHECK (downloaded >= 0),
    PRIMARY KEY (download_id, idx)
);
