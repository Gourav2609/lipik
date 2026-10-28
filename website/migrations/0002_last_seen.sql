-- When each install last checked in (epoch ms), for "using it right now".
ALTER TABLE installs ADD COLUMN last_seen INTEGER NOT NULL DEFAULT 0;
