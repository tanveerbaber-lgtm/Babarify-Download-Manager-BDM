-- 0004: downloads sent from the browser extension.

-- Page that linked to the file; sent as the Referer header, which some
-- servers require.
ALTER TABLE downloads ADD COLUMN referrer TEXT;
