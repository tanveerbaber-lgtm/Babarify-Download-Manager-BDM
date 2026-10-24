-- 0003: video/audio downloads handled by yt-dlp.

ALTER TABLE downloads ADD COLUMN kind TEXT NOT NULL DEFAULT 'file'
    CHECK (kind IN ('file', 'media'));
ALTER TABLE downloads ADD COLUMN title         TEXT;  -- media title from the site
ALTER TABLE downloads ADD COLUMN media_options TEXT;  -- JSON: quality / container / audio format
