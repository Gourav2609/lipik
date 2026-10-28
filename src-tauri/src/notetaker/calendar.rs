use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use specta::Type;

use super::Attendee;

#[derive(Clone, Debug, Serialize, Deserialize, Type, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum CalendarAccess {
    NotDetermined,
    Denied,
    Restricted,
    WriteOnly,
    FullAccess,
    Unsupported,
}

#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct CalendarEvent {
    /// Unique per occurrence: the EventKit event id plus the start time.
    pub id: String,
    pub title: String,
    pub start_ms: i64,
    pub end_ms: i64,
    pub all_day: bool,
    pub attendees: Vec<Attendee>,
    pub organizer: Option<Attendee>,
    pub meeting_url: Option<String>,
    pub location: Option<String>,
    pub calendar: Option<String>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, Type, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum CalendarSourceKind {
    Google,
    Exchange,
    Icloud,
    Caldav,
    Local,
    Subscribed,
    Birthdays,
}

/// An account in the Calendar app (EventKit source) and its event calendars.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct CalendarSource {
    pub id: String,
    pub title: String,
    pub kind: CalendarSourceKind,
    pub calendars: Vec<String>,
}

static MEETING_URL: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r#"https://(?:[\w-]+\.)?(?:zoom\.us/(?:j|my|w)/[^\s"<>]+|meet\.google\.com/[a-z]{3}-[a-z]{4}-[a-z]{3}[^\s"<>]*|teams\.microsoft\.com/l/meetup-join/[^\s"<>]+|teams\.live\.com/meet/[^\s"<>]+|[\w-]+\.webex\.com/[^\s"<>]+|app\.slack\.com/huddle/[^\s"<>]+|facetime\.apple\.com/join[^\s"<>]+)"#,
    )
    .unwrap()
});

pub fn find_meeting_url(texts: &[Option<String>]) -> Option<String> {
    texts
        .iter()
        .flatten()
        .find_map(|t| MEETING_URL.find(t).map(|m| m.as_str().to_string()))
}

/// The event happening now (or starting within `lead_ms`), preferring the one
/// that started most recently.
pub fn current_event(
    events: &[CalendarEvent],
    now_ms: i64,
    lead_ms: i64,
) -> Option<&CalendarEvent> {
    events
        .iter()
        .filter(|e| !e.all_day && e.start_ms - lead_ms <= now_ms && now_ms < e.end_ms)
        .max_by_key(|e| e.start_ms)
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use block2::RcBlock;
    use objc2::rc::{autoreleasepool, Retained};
    use objc2::runtime::Bool;
    use objc2::{sel, AnyThread};
    use objc2_event_kit::{
        EKAuthorizationStatus, EKEntityType, EKEvent, EKEventStore, EKParticipant,
        EKParticipantType, EKSourceType,
    };
    use objc2_foundation::{NSDate, NSError, NSObjectProtocol};
    use std::sync::mpsc::channel;
    use std::time::Duration;

    thread_local! {
        /// One store per thread: creating and releasing one is slow.
        static STORE: Retained<EKEventStore> = unsafe { EKEventStore::init(EKEventStore::alloc()) };
    }

    pub fn access() -> CalendarAccess {
        let status = unsafe { EKEventStore::authorizationStatusForEntityType(EKEntityType::Event) };
        match status {
            EKAuthorizationStatus::NotDetermined => CalendarAccess::NotDetermined,
            EKAuthorizationStatus::Restricted => CalendarAccess::Restricted,
            EKAuthorizationStatus::Denied => CalendarAccess::Denied,
            EKAuthorizationStatus::WriteOnly => CalendarAccess::WriteOnly,
            _ => CalendarAccess::FullAccess,
        }
    }

    /// Shows the system prompt when access was never asked for, and waits for
    /// the answer.
    pub fn request_access() -> CalendarAccess {
        if access() != CalendarAccess::NotDetermined {
            return access();
        }
        let (tx, rx) = channel::<bool>();
        let block = RcBlock::new(move |granted: Bool, _error: *mut NSError| {
            let _ = tx.send(granted.as_bool());
        });
        autoreleasepool(|_| {
            STORE.with(|store| unsafe {
                if store.respondsToSelector(sel!(requestFullAccessToEventsWithCompletion:)) {
                    store.requestFullAccessToEventsWithCompletion(RcBlock::as_ptr(&block));
                } else {
                    #[allow(deprecated)]
                    store.requestAccessToEntityType_completion(
                        EKEntityType::Event,
                        RcBlock::as_ptr(&block),
                    );
                }
            })
        });
        let _ = rx.recv_timeout(Duration::from_secs(300));
        STORE.with(|store| unsafe { store.reset() });
        access()
    }

    fn ms(date: &NSDate) -> i64 {
        (date.timeIntervalSince1970() * 1000.0) as i64
    }

    fn attendee(p: &EKParticipant) -> Attendee {
        unsafe {
            let url = p.URL().absoluteString().map(|s| s.to_string());
            let email = url
                .as_deref()
                .and_then(|u| u.strip_prefix("mailto:"))
                .map(|e| e.to_string());
            Attendee {
                name: p.name().map(|n| n.to_string()),
                email,
                is_me: p.isCurrentUser(),
                not_person: matches!(
                    p.participantType(),
                    EKParticipantType::Room
                        | EKParticipantType::Resource
                        | EKParticipantType::Group
                ),
            }
        }
    }

    fn event(e: &EKEvent) -> CalendarEvent {
        unsafe {
            let start_ms = ms(&e.startDate());
            let base_id = e
                .eventIdentifier()
                .map(|s| s.to_string())
                .unwrap_or_else(|| e.calendarItemIdentifier().to_string());
            let url = e
                .URL()
                .and_then(|u| u.absoluteString())
                .map(|s| s.to_string());
            let location = e.location().map(|s| s.to_string());
            let notes = e.notes().map(|s| s.to_string());
            CalendarEvent {
                id: format!("{base_id}@{start_ms}"),
                title: e.title().to_string(),
                start_ms,
                end_ms: ms(&e.endDate()),
                all_day: e.isAllDay(),
                attendees: e
                    .attendees()
                    .map(|list| list.iter().map(|p| attendee(&p)).collect())
                    .unwrap_or_default(),
                organizer: e.organizer().map(|p| attendee(&p)),
                meeting_url: find_meeting_url(&[url, location.clone(), notes]),
                location,
                calendar: e.calendar().map(|c| c.title().to_string()),
            }
        }
    }

    /// Events between the two times. Empty unless full access was granted;
    /// never prompts.
    pub fn events_between(start_ms: i64, end_ms: i64) -> Vec<CalendarEvent> {
        if access() != CalendarAccess::FullAccess {
            return Vec::new();
        }
        autoreleasepool(|_| {
            STORE.with(|store| unsafe {
                let start = NSDate::dateWithTimeIntervalSince1970(start_ms as f64 / 1000.0);
                let end = NSDate::dateWithTimeIntervalSince1970(end_ms as f64 / 1000.0);
                let predicate =
                    store.predicateForEventsWithStartDate_endDate_calendars(&start, &end, None);
                let events: Retained<_> = store.eventsMatchingPredicate(&predicate);
                let mut out: Vec<CalendarEvent> = events.iter().map(|e| event(&e)).collect();
                out.sort_by_key(|e| e.start_ms);
                out
            })
        })
    }

    fn source_kind(kind: EKSourceType, title: &str) -> CalendarSourceKind {
        let lower = title.to_lowercase();
        match kind {
            EKSourceType::Exchange => CalendarSourceKind::Exchange,
            EKSourceType::MobileMe => CalendarSourceKind::Icloud,
            EKSourceType::Local => CalendarSourceKind::Local,
            EKSourceType::Subscribed => CalendarSourceKind::Subscribed,
            EKSourceType::Birthdays => CalendarSourceKind::Birthdays,
            _ if lower == "icloud" => CalendarSourceKind::Icloud,
            _ if lower.contains("google") || lower.ends_with("gmail.com") => {
                CalendarSourceKind::Google
            }
            _ => CalendarSourceKind::Caldav,
        }
    }

    /// Accounts with event calendars. Uses a fresh store: a thread's cached
    /// store made before access was granted sees no calendars.
    pub fn sources() -> Vec<CalendarSource> {
        if access() != CalendarAccess::FullAccess {
            return Vec::new();
        }
        autoreleasepool(|_| unsafe {
            let store = EKEventStore::init(EKEventStore::alloc());
            let mut out: Vec<CalendarSource> = Vec::new();
            for cal in store.calendarsForEntityType(EKEntityType::Event).iter() {
                let Some(src) = cal.source() else { continue };
                let id = src.sourceIdentifier().to_string();
                let name = cal.title().to_string();
                match out.iter_mut().find(|s| s.id == id) {
                    Some(s) => s.calendars.push(name),
                    None => {
                        let title = src.title().to_string();
                        out.push(CalendarSource {
                            kind: source_kind(src.sourceType(), &title),
                            id,
                            title,
                            calendars: vec![name],
                        });
                    }
                }
            }
            for s in &mut out {
                s.calendars.sort();
            }
            out.sort_by(|a, b| (a.kind, &a.title).cmp(&(b.kind, &b.title)));
            out
        })
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use super::*;

    pub fn access() -> CalendarAccess {
        CalendarAccess::Unsupported
    }

    pub fn request_access() -> CalendarAccess {
        CalendarAccess::Unsupported
    }

    pub fn events_between(_start_ms: i64, _end_ms: i64) -> Vec<CalendarEvent> {
        Vec::new()
    }

    pub fn sources() -> Vec<CalendarSource> {
        Vec::new()
    }
}

pub use imp::{access, events_between, request_access, sources};

/// Today and tomorrow, local time.
pub fn upcoming_window() -> (i64, i64) {
    use chrono::{Duration, Local, TimeZone};
    let today = Local::now().date_naive().and_hms_opt(0, 0, 0).unwrap();
    let start = Local
        .from_local_datetime(&today)
        .earliest()
        .unwrap_or_else(Local::now);
    let end = start + Duration::days(2);
    (start.timestamp_millis(), end.timestamp_millis())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_meeting_links() {
        let notes = Some("Join: https://us02web.zoom.us/j/123456789?pwd=abc more text".to_string());
        assert_eq!(
            find_meeting_url(&[None, notes]).as_deref(),
            Some("https://us02web.zoom.us/j/123456789?pwd=abc")
        );
        let meet = Some("https://meet.google.com/abc-defg-hij".to_string());
        assert!(find_meeting_url(&[meet]).is_some());
        assert!(find_meeting_url(&[Some("https://example.com".into())]).is_none());
    }
}
