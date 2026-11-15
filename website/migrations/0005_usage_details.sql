-- What app versions from 0.1.17 also send. NULL: an older version, or a user
-- who has not yet seen the note that says these are sent.
-- Counts since 0.1.17, as last reported:
ALTER TABLE installs ADD COLUMN dictations INTEGER;
ALTER TABLE installs ADD COLUMN words INTEGER;
ALTER TABLE installs ADD COLUMN meetings INTEGER;
ALTER TABLE installs ADD COLUMN meeting_minutes INTEGER;
-- Settings: the speech model's name ("custom" for the user's own file), the
-- speech engine, the language, the clean-up engine (NULL also when it is
-- off) and whether meeting notes are on.
ALTER TABLE installs ADD COLUMN model TEXT;
ALTER TABLE installs ADD COLUMN engine TEXT;
ALTER TABLE installs ADD COLUMN language TEXT;
ALTER TABLE installs ADD COLUMN cleanup TEXT;
ALTER TABLE installs ADD COLUMN meeting_notes INTEGER;

-- The counts as they stood at the last check-in of each day, so the use in
-- any stretch of days is a difference.
ALTER TABLE active_days ADD COLUMN dictations INTEGER;
ALTER TABLE active_days ADD COLUMN words INTEGER;
ALTER TABLE active_days ADD COLUMN meetings INTEGER;
ALTER TABLE active_days ADD COLUMN meeting_minutes INTEGER;
