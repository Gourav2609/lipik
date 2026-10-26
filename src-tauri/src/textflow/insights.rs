//! Totals for the Insights page, computed from history rows.

use crate::managers::history::DictationStatRow;
use chrono::{DateTime, Local, NaiveDate};
use serde::Serialize;
use specta::Type;
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Serialize, Debug, Clone, Type)]
pub struct DailyWords {
    /// Local calendar day, `YYYY-MM-DD`.
    pub date: String,
    pub words: i64,
}

#[derive(Serialize, Debug, Clone, Type)]
pub struct Insights {
    /// Words per minute of recorded audio; none until a dictation with a
    /// recorded duration exists.
    pub wpm: Option<f64>,
    pub total_words: i64,
    /// `words_corrected + dictionary_fixes`.
    pub fixes_total: i64,
    /// Words changed by clean-up and transforms.
    pub words_corrected: i64,
    /// Replacements applied plus dictionary words in the final text.
    pub dictionary_fixes: i64,
    /// Dictations per app type: ai, email, work, personal, document, other.
    pub by_category: HashMap<String, i64>,
    /// Distinct apps dictated into.
    pub apps_used: i64,
    pub streak_days: i64,
    pub longest_streak: i64,
    pub daily_words: Vec<DailyWords>,
}

pub fn compute(rows: &[DictationStatRow], today: NaiveDate) -> Insights {
    let mut by_day: BTreeMap<NaiveDate, i64> = BTreeMap::new();
    let mut by_category: HashMap<String, i64> = ["ai", "email", "work", "personal", "document", "other"]
        .iter()
        .map(|k| (k.to_string(), 0))
        .collect();
    let mut apps = HashSet::new();
    let (mut total_words, mut timed_words, mut timed_ms) = (0i64, 0i64, 0i64);
    let (mut words_corrected, mut dictionary_fixes) = (0i64, 0i64);

    for row in rows {
        let words = row
            .words
            .unwrap_or_else(|| super::text::count_words(&row.text) as i64);
        if words == 0 {
            continue;
        }
        total_words += words;
        if let Some(day) = DateTime::from_timestamp(row.timestamp, 0) {
            *by_day.entry(day.with_timezone(&Local).date_naive()).or_default() += words;
        }
        if let Some(ms) = row.duration_ms.filter(|ms| *ms > 0) {
            timed_words += words;
            timed_ms += ms;
        }
        words_corrected += row.fixes.unwrap_or(0);
        dictionary_fixes += row.dictionary_fixes.unwrap_or(0);
        if let Some(app_type) = &row.app_type {
            *by_category.entry(app_type.clone()).or_default() += 1;
        }
        if let Some(bundle) = &row.app_bundle_id {
            apps.insert(bundle.clone());
        }
    }

    let mut cursor = if by_day.contains_key(&today) {
        Some(today)
    } else {
        today.pred_opt()
    };
    let mut streak_days = 0;
    while let Some(day) = cursor.filter(|d| by_day.contains_key(d)) {
        streak_days += 1;
        cursor = day.pred_opt();
    }

    let (mut longest_streak, mut run, mut prev) = (0i64, 0i64, None::<NaiveDate>);
    for day in by_day.keys() {
        run = if prev.and_then(|p| p.succ_opt()) == Some(*day) {
            run + 1
        } else {
            1
        };
        longest_streak = longest_streak.max(run);
        prev = Some(*day);
    }

    Insights {
        wpm: (timed_ms > 0).then(|| timed_words as f64 * 60_000.0 / timed_ms as f64),
        total_words,
        fixes_total: words_corrected + dictionary_fixes,
        words_corrected,
        dictionary_fixes,
        by_category,
        apps_used: apps.len() as i64,
        streak_days,
        longest_streak,
        daily_words: by_day
            .into_iter()
            .map(|(date, words)| DailyWords {
                date: date.format("%Y-%m-%d").to_string(),
                words,
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn row(day: NaiveDate, words: i64, app_type: &str) -> DictationStatRow {
        let ts = Local
            .from_local_datetime(&day.and_hms_opt(12, 0, 0).unwrap())
            .unwrap()
            .timestamp();
        DictationStatRow {
            timestamp: ts,
            text: String::new(),
            words: Some(words),
            duration_ms: Some(words * 600),
            fixes: Some(1),
            dictionary_fixes: Some(2),
            app_bundle_id: Some(format!("app.{app_type}")),
            app_type: Some(app_type.to_string()),
        }
    }

    #[test]
    fn streaks_totals_and_wpm() {
        let d = |n: u32| NaiveDate::from_ymd_opt(2026, 10, n).unwrap();
        let rows = vec![
            row(d(1), 10, "work"),
            row(d(2), 10, "work"),
            row(d(3), 10, "email"),
            row(d(20), 5, "other"),
            row(d(21), 5, "other"),
        ];
        let insights = compute(&rows, d(22));
        assert_eq!(insights.total_words, 40);
        assert_eq!(insights.streak_days, 2);
        assert_eq!(insights.longest_streak, 3);
        assert_eq!(insights.wpm, Some(100.0));
        assert_eq!(insights.words_corrected, 5);
        assert_eq!(insights.fixes_total, 15);
        assert_eq!(insights.by_category["work"], 2);
        assert_eq!(insights.apps_used, 3);
        assert_eq!(insights.daily_words.first().unwrap().date, "2026-10-01");
    }

    #[test]
    fn streak_breaks_after_a_missed_day() {
        let d = |n: u32| NaiveDate::from_ymd_opt(2026, 10, n).unwrap();
        let insights = compute(&[row(d(1), 3, "ai")], d(3));
        assert_eq!(insights.streak_days, 0);
        assert_eq!(insights.wpm, Some(100.0));
    }
}
