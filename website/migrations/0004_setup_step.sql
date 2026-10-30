-- How far setup got, the furthest the install has reported:
-- 0 unknown (older app versions don't send it), 1 installed,
-- 2 permissions, 3 model_ready, 4 first_dictation.
ALTER TABLE installs ADD COLUMN step INTEGER NOT NULL DEFAULT 0;

-- Retention looks up each install's active days.
CREATE INDEX active_days_id ON active_days (id, day);
