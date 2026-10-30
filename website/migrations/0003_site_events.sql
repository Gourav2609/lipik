-- Clicks on the .dmg download links and copies of the Terminal install
-- command on the website, counted per day and page. Nothing about the visitor.
CREATE TABLE site_events (
  day TEXT NOT NULL,
  kind TEXT NOT NULL,
  page TEXT NOT NULL,
  n INTEGER NOT NULL,
  PRIMARY KEY (day, kind, page)
);
