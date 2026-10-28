-- One row per install: a random ID the app makes, never tied to a person.
CREATE TABLE installs (
  id TEXT PRIMARY KEY,
  first_day TEXT NOT NULL,
  last_day TEXT NOT NULL,
  version TEXT NOT NULL,
  os TEXT NOT NULL,
  arch TEXT NOT NULL,
  country TEXT NOT NULL
);

-- One row per install per day it ran.
CREATE TABLE active_days (
  day TEXT NOT NULL,
  id TEXT NOT NULL,
  version TEXT NOT NULL,
  PRIMARY KEY (day, id)
);

-- Emails people typed in setup.
CREATE TABLE emails (
  email TEXT PRIMARY KEY,
  install_id TEXT NOT NULL,
  updates INTEGER NOT NULL,
  first_day TEXT NOT NULL,
  last_day TEXT NOT NULL
);

-- Times install.sh was downloaded, per day.
CREATE TABLE installer_runs (
  day TEXT PRIMARY KEY,
  runs INTEGER NOT NULL
);
