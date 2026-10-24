-- 0005: queues, priorities, categories, speed limits and automatic retries.

CREATE TABLE queues (
    id         TEXT    PRIMARY KEY NOT NULL,
    name       TEXT    NOT NULL,
    -- JSON schedule: {"enabled":bool,"start":"HH:MM","stop":"HH:MM"|null,"days":[0..6]}
    schedule   TEXT    NOT NULL DEFAULT '{"enabled":false,"start":"00:00","stop":null,"days":[0,1,2,3,4,5,6]}',
    -- Stopped queues start nothing until started again.
    running    INTEGER NOT NULL DEFAULT 1,
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);
INSERT INTO queues (id, name, sort_order, created_at) VALUES ('main', 'Main', 0, 0);

CREATE TABLE categories (
    id         TEXT    PRIMARY KEY NOT NULL,
    name       TEXT    NOT NULL,
    extensions TEXT    NOT NULL,          -- comma-separated, lowercase, no dots
    folder     TEXT,                      -- absolute path; NULL = <default folder>\<name>
    sort_order INTEGER NOT NULL DEFAULT 0
);
INSERT INTO categories (id, name, extensions, sort_order) VALUES
    ('videos',     'Videos',     'mp4,mkv,webm,avi,mov,m4v,wmv,flv,ts,3gp', 1),
    ('music',      'Music',      'mp3,m4a,flac,wav,ogg,opus,aac,wma,alac', 2),
    ('documents',  'Documents',  'pdf,doc,docx,xls,xlsx,ppt,pptx,txt,rtf,odt,epub,csv', 3),
    ('compressed', 'Compressed', 'zip,rar,7z,tar,gz,bz2,xz,iso,img,cab', 4),
    ('programs',   'Programs',   'exe,msi,msix,appx,apk,dmg,deb,rpm,bat', 5);

ALTER TABLE downloads ADD COLUMN queue_id    TEXT    NOT NULL DEFAULT 'main';
ALTER TABLE downloads ADD COLUMN priority    INTEGER NOT NULL DEFAULT 0 CHECK (priority IN (-1, 0, 1));
ALTER TABLE downloads ADD COLUMN position    INTEGER NOT NULL DEFAULT 0; -- order within a queue
ALTER TABLE downloads ADD COLUMN category    TEXT;                       -- category id, NULL = other
ALTER TABLE downloads ADD COLUMN speed_limit INTEGER CHECK (speed_limit IS NULL OR speed_limit > 0); -- bytes/s
ALTER TABLE downloads ADD COLUMN auto_retries INTEGER NOT NULL DEFAULT 0;  -- automatic retries used
ALTER TABLE downloads ADD COLUMN retry_at     INTEGER;                     -- next automatic retry (unix ms)

UPDATE downloads SET position = created_at;
CREATE INDEX idx_downloads_queue ON downloads (status, queue_id, priority DESC, position);
CREATE INDEX idx_downloads_url ON downloads (url);
