-- 0006: virus scan result of a finished download.
-- NULL = not scanned; 'scanning', 'clean', 'threat' or 'unavailable'.
ALTER TABLE downloads ADD COLUMN scan_result TEXT;
