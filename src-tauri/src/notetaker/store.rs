use anyhow::{anyhow, Result};
use rusqlite::{params, Connection, OptionalExtension, Row};
use rusqlite_migration::{Migrations, M};
use std::path::Path;
use std::sync::Mutex;

use super::{
    now_ms, Attendee, ChatMessage, ChatThread, Citation, FolderInfo, Meeting, MeetingDetail,
    MeetingStatus, NoteSummary, NotetakerFilter, Segment, SummaryStatus,
};

static MIGRATIONS: &[M] = &[
    M::up(
    "CREATE TABLE meetings (
        id TEXT PRIMARY KEY,
        title TEXT NOT NULL,
        title_auto INTEGER NOT NULL DEFAULT 0,
        start_ms INTEGER NOT NULL,
        end_ms INTEGER,
        app TEXT,
        bundle_id TEXT,
        calendar_event_id TEXT,
        attendees_json TEXT NOT NULL DEFAULT '[]',
        folder TEXT,
        status TEXT NOT NULL,
        summary_status TEXT NOT NULL DEFAULT 'pending',
        summary_error TEXT,
        mic_offset_ms INTEGER,
        system_offset_ms INTEGER,
        updated_ms INTEGER NOT NULL
    );
    CREATE INDEX meetings_start ON meetings(start_ms DESC);
    CREATE TABLE segments (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        meeting_id TEXT NOT NULL,
        pass TEXT NOT NULL,
        source TEXT NOT NULL,
        speaker TEXT NOT NULL,
        start_ms INTEGER NOT NULL,
        end_ms INTEGER NOT NULL,
        text TEXT NOT NULL
    );
    CREATE INDEX segments_meeting ON segments(meeting_id, start_ms);
    CREATE TABLE my_thoughts (
        meeting_id TEXT PRIMARY KEY,
        markdown TEXT NOT NULL,
        updated_ms INTEGER NOT NULL
    );
    CREATE TABLE summaries (
        meeting_id TEXT PRIMARY KEY,
        json TEXT NOT NULL,
        model TEXT NOT NULL,
        created_ms INTEGER NOT NULL
    );
    CREATE TABLE chat_messages (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        thread_id TEXT NOT NULL,
        meeting_id TEXT,
        role TEXT NOT NULL,
        content TEXT NOT NULL,
        citations_json TEXT NOT NULL DEFAULT '[]',
        created_ms INTEGER NOT NULL
    );
    CREATE INDEX chat_thread ON chat_messages(thread_id, id);",
    ),
    M::up(
        "ALTER TABLE meetings ADD COLUMN process_attempts INTEGER NOT NULL DEFAULT 0;
    ALTER TABLE summaries ADD COLUMN search_text TEXT NOT NULL DEFAULT '';",
    ),
];

const MEETING_COLUMNS: &str = "id, title, title_auto, start_ms, end_ms, app, bundle_id, calendar_event_id, attendees_json, folder, status, summary_status, summary_error, mic_offset_ms, system_offset_ms";

pub struct Store {
    conn: Mutex<Connection>,
}

/// The words of the notes, without the JSON keys, for search.
fn summary_text(summary: &NoteSummary) -> String {
    let mut parts: Vec<&str> = summary.title.as_deref().into_iter().collect();
    parts.push(&summary.overview);
    for section in &summary.sections {
        parts.push(&section.heading);
        parts.extend(section.bullets.iter().map(String::as_str));
    }
    parts.extend(summary.key_points.iter().map(String::as_str));
    parts.extend(summary.decisions.iter().map(String::as_str));
    for item in &summary.action_items {
        parts.push(&item.text);
        parts.extend(item.owner.as_deref());
    }
    parts.join("\n")
}

fn meeting_from_row(row: &Row) -> rusqlite::Result<Meeting> {
    let attendees: String = row.get(8)?;
    let status: String = row.get(10)?;
    let summary_status: String = row.get(11)?;
    Ok(Meeting {
        id: row.get(0)?,
        title: row.get(1)?,
        title_auto: row.get::<_, i64>(2)? != 0,
        start_ms: row.get(3)?,
        end_ms: row.get(4)?,
        app: row.get(5)?,
        bundle_id: row.get(6)?,
        calendar_event_id: row.get(7)?,
        attendees: serde_json::from_str::<Vec<Attendee>>(&attendees).unwrap_or_default(),
        folder: row.get(9)?,
        status: MeetingStatus::parse(&status),
        summary_status: SummaryStatus::parse(&summary_status),
        summary_error: row.get(12)?,
        mic_offset_ms: row.get(13)?,
        system_offset_ms: row.get(14)?,
    })
}

fn segment_from_row(row: &Row) -> rusqlite::Result<Segment> {
    Ok(Segment {
        source: row.get(0)?,
        speaker: row.get(1)?,
        start_ms: row.get(2)?,
        end_ms: row.get(3)?,
        text: row.get(4)?,
    })
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        let mut conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Migrations::from_slice(MIGRATIONS)
            .to_latest(&mut conn)
            .map_err(|e| anyhow!("notetaker migrations failed: {e}"))?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn insert_meeting(&self, m: &Meeting) -> Result<()> {
        self.conn().execute(
            "INSERT INTO meetings (id, title, title_auto, start_ms, end_ms, app, bundle_id, calendar_event_id, attendees_json, folder, status, summary_status, updated_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                m.id,
                m.title,
                m.title_auto as i64,
                m.start_ms,
                m.end_ms,
                m.app,
                m.bundle_id,
                m.calendar_event_id,
                serde_json::to_string(&m.attendees)?,
                m.folder,
                m.status.as_str(),
                m.summary_status.as_str(),
                now_ms(),
            ],
        )?;
        Ok(())
    }

    /// The call app a meeting was linked to, once detection found it.
    pub fn set_call(&self, id: &str, app: &str, bundle_id: &str) -> Result<()> {
        self.conn().execute(
            "UPDATE meetings SET app = ?2, bundle_id = ?3, updated_ms = ?4 WHERE id = ?1",
            params![id, app, bundle_id, now_ms()],
        )?;
        Ok(())
    }

    pub fn meeting(&self, id: &str) -> Result<Option<Meeting>> {
        let sql = format!("SELECT {MEETING_COLUMNS} FROM meetings WHERE id = ?1");
        Ok(self
            .conn()
            .query_row(&sql, params![id], meeting_from_row)
            .optional()?)
    }

    pub fn list(&self, filter: &NotetakerFilter) -> Result<Vec<Meeting>> {
        let mut sql = format!("SELECT {MEETING_COLUMNS} FROM meetings m WHERE 1 = 1");
        let mut args: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(folder) = &filter.folder {
            args.push(Box::new(folder.clone()));
            sql.push_str(&format!(" AND folder = ?{}", args.len()));
        }
        if let Some(query) = filter.query.as_deref().map(str::trim).filter(|q| !q.is_empty()) {
            args.push(Box::new(format!("%{query}%")));
            let n = args.len();
            sql.push_str(&format!(
                " AND (title LIKE ?{n}
                   OR EXISTS (SELECT 1 FROM segments s WHERE s.meeting_id = m.id AND s.text LIKE ?{n})
                   OR EXISTS (SELECT 1 FROM my_thoughts t WHERE t.meeting_id = m.id AND t.markdown LIKE ?{n})
                   OR EXISTS (SELECT 1 FROM summaries u WHERE u.meeting_id = m.id AND u.search_text LIKE ?{n}))"
            ));
        }
        if let Some(after) = filter.after_ms {
            args.push(Box::new(after));
            sql.push_str(&format!(" AND start_ms >= ?{}", args.len()));
        }
        if let Some(before) = filter.before_ms {
            args.push(Box::new(before));
            sql.push_str(&format!(" AND start_ms < ?{}", args.len()));
        }
        sql.push_str(" ORDER BY start_ms DESC");
        args.push(Box::new(filter.limit.unwrap_or(100) as i64));
        sql.push_str(&format!(" LIMIT ?{}", args.len()));
        args.push(Box::new(filter.offset.unwrap_or(0) as i64));
        sql.push_str(&format!(" OFFSET ?{}", args.len()));

        let conn = self.conn();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(
            rusqlite::params_from_iter(args.iter().map(|a| a.as_ref())),
            meeting_from_row,
        )?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn meetings_with_status(&self, statuses: &[MeetingStatus]) -> Result<Vec<Meeting>> {
        let mut out = Vec::new();
        for status in statuses {
            let sql = format!("SELECT {MEETING_COLUMNS} FROM meetings WHERE status = ?1");
            let conn = self.conn();
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(params![status.as_str()], meeting_from_row)?;
            out.extend(rows.collect::<rusqlite::Result<Vec<_>>>()?);
        }
        Ok(out)
    }

    pub fn detail(&self, id: &str) -> Result<Option<MeetingDetail>> {
        let Some(meeting) = self.meeting(id)? else {
            return Ok(None);
        };
        Ok(Some(MeetingDetail {
            segments: self.segments(id)?,
            thoughts: self.thoughts(id)?,
            summary: self.summary(id)?,
            meeting,
        }))
    }

    /// Final-pass segments when they exist, otherwise the live ones.
    pub fn segments(&self, id: &str) -> Result<Vec<Segment>> {
        let conn = self.conn();
        let has_final: bool = conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM segments WHERE meeting_id = ?1 AND pass = 'final')",
            params![id],
            |r| r.get(0),
        )?;
        let pass = if has_final { "final" } else { "live" };
        let mut stmt = conn.prepare(
            "SELECT source, speaker, start_ms, end_ms, text FROM segments
             WHERE meeting_id = ?1 AND pass = ?2 ORDER BY start_ms, id",
        )?;
        let rows = stmt.query_map(params![id, pass], segment_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn add_live_segment(&self, meeting_id: &str, s: &Segment) -> Result<()> {
        self.conn().execute(
            "INSERT INTO segments (meeting_id, pass, source, speaker, start_ms, end_ms, text)
             VALUES (?1, 'live', ?2, ?3, ?4, ?5, ?6)",
            params![meeting_id, s.source, s.speaker, s.start_ms, s.end_ms, s.text],
        )?;
        Ok(())
    }

    pub fn replace_final_segments(&self, meeting_id: &str, segments: &[Segment]) -> Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM segments WHERE meeting_id = ?1", params![meeting_id])?;
        for s in segments {
            tx.execute(
                "INSERT INTO segments (meeting_id, pass, source, speaker, start_ms, end_ms, text)
                 VALUES (?1, 'final', ?2, ?3, ?4, ?5, ?6)",
                params![meeting_id, s.source, s.speaker, s.start_ms, s.end_ms, s.text],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn thoughts(&self, id: &str) -> Result<String> {
        Ok(self
            .conn()
            .query_row(
                "SELECT markdown FROM my_thoughts WHERE meeting_id = ?1",
                params![id],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or_default())
    }

    pub fn save_thoughts(&self, id: &str, markdown: &str) -> Result<()> {
        self.conn().execute(
            "INSERT INTO my_thoughts (meeting_id, markdown, updated_ms) VALUES (?1, ?2, ?3)
             ON CONFLICT(meeting_id) DO UPDATE SET markdown = excluded.markdown, updated_ms = excluded.updated_ms",
            params![id, markdown, now_ms()],
        )?;
        Ok(())
    }

    pub fn summary(&self, id: &str) -> Result<Option<NoteSummary>> {
        let json: Option<String> = self
            .conn()
            .query_row(
                "SELECT json FROM summaries WHERE meeting_id = ?1",
                params![id],
                |r| r.get(0),
            )
            .optional()?;
        Ok(json.and_then(|j| serde_json::from_str(&j).ok()))
    }

    pub fn save_summary(&self, id: &str, summary: &NoteSummary, model: &str) -> Result<()> {
        self.conn().execute(
            "INSERT INTO summaries (meeting_id, json, model, created_ms, search_text) VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(meeting_id) DO UPDATE SET json = excluded.json, model = excluded.model,
                created_ms = excluded.created_ms, search_text = excluded.search_text",
            params![id, serde_json::to_string(summary)?, model, now_ms(), summary_text(summary)],
        )?;
        Ok(())
    }

    pub fn set_status(&self, id: &str, status: MeetingStatus) -> Result<()> {
        self.conn().execute(
            "UPDATE meetings SET status = ?2, updated_ms = ?3 WHERE id = ?1",
            params![id, status.as_str(), now_ms()],
        )?;
        Ok(())
    }

    pub fn set_summary_status(
        &self,
        id: &str,
        status: SummaryStatus,
        error: Option<&str>,
    ) -> Result<()> {
        self.conn().execute(
            "UPDATE meetings SET summary_status = ?2, summary_error = ?3, updated_ms = ?4 WHERE id = ?1",
            params![id, status.as_str(), error, now_ms()],
        )?;
        Ok(())
    }

    pub fn set_end(&self, id: &str, end_ms: i64) -> Result<()> {
        self.conn().execute(
            "UPDATE meetings SET end_ms = ?2, updated_ms = ?3 WHERE id = ?1",
            params![id, end_ms, now_ms()],
        )?;
        Ok(())
    }

    pub fn set_offsets(&self, id: &str, mic: Option<i64>, system: Option<i64>) -> Result<()> {
        self.conn().execute(
            "UPDATE meetings SET mic_offset_ms = COALESCE(?2, mic_offset_ms),
                system_offset_ms = COALESCE(?3, system_offset_ms), updated_ms = ?4 WHERE id = ?1",
            params![id, mic, system, now_ms()],
        )?;
        Ok(())
    }

    /// A title from the user: notes will no longer replace it.
    pub fn rename(&self, id: &str, title: &str) -> Result<()> {
        let changed = self.conn().execute(
            "UPDATE meetings SET title = ?2, title_auto = 0, updated_ms = ?3 WHERE id = ?1",
            params![id, title, now_ms()],
        )?;
        if changed == 0 {
            return Err(anyhow!("No meeting with id {id}"));
        }
        Ok(())
    }

    /// A suggested title, applied only while the title is still a made-up one.
    pub fn set_auto_title(&self, id: &str, title: &str) -> Result<()> {
        self.conn().execute(
            "UPDATE meetings SET title = ?2, updated_ms = ?3 WHERE id = ?1 AND title_auto = 1",
            params![id, title, now_ms()],
        )?;
        Ok(())
    }

    /// Counts a processing run; returns how many runs were started.
    pub fn add_process_attempt(&self, id: &str) -> Result<i64> {
        Ok(self.conn().query_row(
            "UPDATE meetings SET process_attempts = process_attempts + 1 WHERE id = ?1
             RETURNING process_attempts",
            params![id],
            |r| r.get(0),
        )?)
    }

    pub fn process_attempts(&self, id: &str) -> Result<i64> {
        Ok(self
            .conn()
            .query_row(
                "SELECT process_attempts FROM meetings WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(0))
    }

    pub fn clear_process_attempts(&self, id: &str) -> Result<()> {
        self.conn().execute(
            "UPDATE meetings SET process_attempts = 0 WHERE id = ?1",
            params![id],
        )?;
        Ok(())
    }

    pub fn move_to_folder(&self, id: &str, folder: Option<&str>) -> Result<()> {
        let changed = self.conn().execute(
            "UPDATE meetings SET folder = ?2, updated_ms = ?3 WHERE id = ?1",
            params![id, folder, now_ms()],
        )?;
        if changed == 0 {
            return Err(anyhow!("No meeting with id {id}"));
        }
        Ok(())
    }

    pub fn folders(&self) -> Result<Vec<FolderInfo>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT folder, COUNT(*) FROM meetings WHERE folder IS NOT NULL GROUP BY folder ORDER BY folder",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(FolderInfo {
                name: r.get(0)?,
                count: r.get::<_, i64>(1)? as u32,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM segments WHERE meeting_id = ?1", params![id])?;
        tx.execute("DELETE FROM my_thoughts WHERE meeting_id = ?1", params![id])?;
        tx.execute("DELETE FROM summaries WHERE meeting_id = ?1", params![id])?;
        tx.execute("DELETE FROM chat_messages WHERE meeting_id = ?1", params![id])?;
        let cited: Vec<(i64, String)> = {
            let mut stmt = tx.prepare(
                "SELECT id, citations_json FROM chat_messages WHERE meeting_id IS NULL AND instr(citations_json, ?1) > 0",
            )?;
            let rows = stmt.query_map(params![format!("\"{id}\"")], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        for (message_id, json) in cited {
            let mut citations: Vec<Citation> = serde_json::from_str(&json).unwrap_or_default();
            citations.retain(|c| c.meeting_id != id);
            tx.execute(
                "UPDATE chat_messages SET citations_json = ?2 WHERE id = ?1",
                params![message_id, serde_json::to_string(&citations)?],
            )?;
        }
        tx.execute("DELETE FROM meetings WHERE id = ?1", params![id])?;
        tx.commit()?;
        Ok(())
    }

    pub fn add_chat_message(&self, msg: &ChatMessage) -> Result<i64> {
        let conn = self.conn();
        conn.execute(
            "INSERT INTO chat_messages (thread_id, meeting_id, role, content, citations_json, created_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                msg.thread_id,
                msg.meeting_id,
                msg.role,
                msg.content,
                serde_json::to_string(&msg.citations)?,
                msg.created_ms,
            ],
        )?;
        Ok(conn.last_insert_rowid())
    }

    pub fn chat_messages(&self, thread_id: &str) -> Result<Vec<ChatMessage>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT id, thread_id, meeting_id, role, content, citations_json, created_ms
             FROM chat_messages WHERE thread_id = ?1 ORDER BY id",
        )?;
        let rows = stmt.query_map(params![thread_id], |r| {
            let citations: String = r.get(5)?;
            Ok(ChatMessage {
                id: r.get(0)?,
                thread_id: r.get(1)?,
                meeting_id: r.get(2)?,
                role: r.get(3)?,
                content: r.get(4)?,
                citations: serde_json::from_str::<Vec<Citation>>(&citations).unwrap_or_default(),
                created_ms: r.get(6)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn chat_threads(&self, meeting_id: Option<&str>) -> Result<Vec<ChatThread>> {
        let conn = self.conn();
        let mut stmt = conn.prepare(
            "SELECT c.thread_id, c.meeting_id, MIN(c.created_ms), MAX(c.created_ms),
                (SELECT content FROM chat_messages f WHERE f.thread_id = c.thread_id AND f.role = 'user' ORDER BY f.id LIMIT 1)
             FROM chat_messages c
             WHERE (?1 IS NULL AND c.meeting_id IS NULL) OR c.meeting_id = ?1
             GROUP BY c.thread_id ORDER BY MAX(c.created_ms) DESC",
        )?;
        let rows = stmt.query_map(params![meeting_id], |r| {
            Ok(ChatThread {
                thread_id: r.get(0)?,
                meeting_id: r.get(1)?,
                started_ms: r.get(2)?,
                updated_ms: r.get(3)?,
                first_question: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}
