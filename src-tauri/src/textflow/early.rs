//! Clean-up while the user is still speaking. Each time the live
//! transcription commits more text, its new sentences that need the model
//! are sent one by one in the background, so at release only the last
//! sentence or two are left. A reply is kept with its exact request and used
//! only when clean-up makes that same request: a sentence the final
//! transcript changed, or one a later correction reaches back into, is sent
//! again at release. Clean-up makes the same requests whether or not any
//! reply came back here; the replies only make it faster.

use super::pipeline::{self, Job, Setup, Target};
use super::{gate, llm, rules, store::TextFlow};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use tauri::{AppHandle, Manager};
use tokio::sync::{watch, Notify};

/// The same limit as a dictation that waits for the model.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);
/// The worker stops after this long without new text; more text starts it
/// again.
const IDLE: Duration = Duration::from_secs(60);
/// Sessions kept at most: one per dictation, so a dictation whose end was
/// missed can't pile up.
const KEPT: usize = 3;

type Finish = Box<dyn Fn(String) -> String + Send + Sync>;

static SESSIONS: Lazy<Mutex<Vec<Arc<Session>>>> = Lazy::new(|| Mutex::new(Vec::new()));

struct Session {
    /// The dictation (its live transcription) this session belongs to.
    id: u64,
    app: AppHandle,
    setup: Setup,
    /// Turns committed text into what the transcription returns (filler
    /// removal and the other steps after the model).
    finish: Finish,
    state: Mutex<State>,
    wake: Notify,
}

#[derive(Default)]
struct State {
    latest: Option<String>,
    heard: Heard,
    closed: bool,
    replies: HashMap<String, String>,
    /// The request being answered and a receiver told when it is done.
    running: Option<(String, watch::Receiver<bool>)>,
    worker: Option<tauri::async_runtime::JoinHandle<()>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// How far the committed text had complete sentences when last handled.
#[derive(Default)]
pub(super) struct Heard {
    text: String,
    done: usize,
}

impl Heard {
    /// Whether `committed` has a complete sentence that wasn't there when
    /// it was last handled: only then can its requests be new.
    pub(super) fn has_new_sentence(&mut self, committed: &str) -> bool {
        if !committed.starts_with(&self.text[..self.done]) {
            self.done = 0;
        }
        let mut sentences = gate::sentence_spans(committed);
        if !committed.trim_end().ends_with(['.', '!', '?']) {
            sentences.pop();
        }
        let Some(&(_, end)) = sentences.last() else {
            return false;
        };
        if end <= self.done {
            return false;
        }
        self.text = committed.to_string();
        self.done = end;
        true
    }
}

/// Starts clean-up while speaking for dictation `id`, whose live
/// transcription will commit text. Does nothing when clean-up is off or
/// sends the whole text anyway (a style other than formal).
pub fn begin(app: &AppHandle, id: u64, finish: impl Fn(String) -> String + Send + Sync + 'static) {
    let Some(setup) = app
        .try_state::<TextFlow>()
        .and_then(|tf| Setup::read(&tf, &Target::live()))
    else {
        return;
    };
    if !setup.sends_parts() {
        return;
    }
    let session = Arc::new(Session {
        id,
        app: app.clone(),
        setup,
        finish: Box::new(finish),
        state: Mutex::new(State::default()),
        wake: Notify::new(),
    });
    let mut sessions = lock(&SESSIONS);
    if let Some(i) = sessions.iter().position(|s| s.id == id) {
        close(&sessions.remove(i));
    }
    while sessions.len() >= KEPT {
        close(&sessions.remove(0));
    }
    sessions.push(session);
}

fn find(id: u64) -> Option<Arc<Session>> {
    lock(&SESSIONS).iter().find(|s| s.id == id).cloned()
}

fn remove(id: u64) -> Option<Arc<Session>> {
    let mut sessions = lock(&SESSIONS);
    let i = sessions.iter().position(|s| s.id == id)?;
    Some(sessions.remove(i))
}

fn close(session: &Session) {
    let mut state = lock(&session.state);
    state.closed = true;
    if let Some(worker) = state.worker.take() {
        worker.abort();
    }
}

/// The text dictation `id`'s live transcription has committed so far.
pub fn committed(id: u64, text: &str) {
    let Some(session) = find(id) else {
        return;
    };
    let mut state = lock(&session.state);
    if state.closed {
        return;
    }
    state.latest = Some(text.to_string());
    if state
        .worker
        .as_ref()
        .is_none_or(|w| w.inner().is_finished())
    {
        let session = Arc::clone(&session);
        state.worker = Some(tauri::async_runtime::spawn(work(session)));
    }
    drop(state);
    session.wake.notify_one();
}

/// Ends dictation `id`'s session, stopping any request it has running.
pub fn stop(id: u64) {
    if let Some(session) = remove(id) {
        close(&session);
    }
}

/// Ends dictation `id`'s session and hands its replies to clean-up.
pub fn take(id: u64) -> Option<Taken> {
    let session = remove(id)?;
    lock(&session.state).closed = true;
    Some(Taken { session })
}

/// A finished session's replies, and the request it may still be running,
/// which is stopped when this is dropped.
pub struct Taken {
    session: Arc<Session>,
}

impl Drop for Taken {
    fn drop(&mut self) {
        close(&self.session);
    }
}

impl Taken {
    /// Stops the running request unless one of `jobs` is waiting for it: the
    /// engine answers one request at a time.
    pub(super) fn keep_only(&self, jobs: &[Job]) {
        let mut state = lock(&self.session.state);
        let needed = state
            .running
            .as_ref()
            .is_some_and(|(key, _)| jobs.iter().any(|j| j.key() == key));
        if !needed {
            if let Some(worker) = state.worker.take() {
                worker.abort();
            }
        }
    }

    /// The reply to `key`, waiting up to `wait` when it is being answered.
    pub(super) async fn reply(&self, key: &str, wait: Duration) -> Option<String> {
        let mut done = {
            let state = lock(&self.session.state);
            if let Some(reply) = state.replies.get(key) {
                return Some(reply.clone());
            }
            match &state.running {
                Some((running, done)) if running == key => done.clone(),
                _ => return None,
            }
        };
        let _ = tokio::time::timeout(wait, done.wait_for(|finished| *finished)).await;
        lock(&self.session.state).replies.get(key).cloned()
    }
}

/// Sends the requests for each new committed text, one at a time, until the
/// session ends or no text comes for `IDLE`.
async fn work(session: Arc<Session>) {
    loop {
        let latest = {
            let mut state = lock(&session.state);
            if state.closed {
                return;
            }
            let latest = state.latest.take();
            latest.map(|text| (state.heard.has_new_sentence(&text), text))
        };
        let Some((new, text)) = latest else {
            if tokio::time::timeout(IDLE, session.wake.notified())
                .await
                .is_err()
            {
                return;
            }
            continue;
        };
        if !new {
            continue;
        }
        // Which Hindi words are fillers depends on the language the final
        // transcript is detected in, so its requests may not match these.
        if rules::hindi_words(&text).len() >= 2 {
            log::debug!("Clean-up while speaking stopped: the dictation looks like Hinglish");
            close(&session);
            return;
        }
        let prepared = pipeline::prepare(&session.app, &(session.finish)(text));
        let endpoint = llm::endpoint(&session.app);
        for job in pipeline::early_jobs(&prepared, &session.setup) {
            let needed = llm::messages_context(&job.messages);
            // Never load or resize the model for a guess; the warm-up at
            // recording start loads it.
            let Some(context) = llm::loaded_context(&endpoint, needed)
                .await
                .filter(|&n| n >= needed)
            else {
                break;
            };
            let key = job.key().to_string();
            let (done, running) = watch::channel(false);
            {
                let mut state = lock(&session.state);
                if state.closed {
                    return;
                }
                if state.replies.contains_key(&key) {
                    continue;
                }
                state.running = Some((key.clone(), running));
            }
            let reply = llm::chat_messages(
                &endpoint,
                &job.messages,
                context,
                REQUEST_TIMEOUT,
                Some(job.limit),
            )
            .await;
            {
                let mut state = lock(&session.state);
                state.running = None;
                match reply {
                    Ok(reply) => {
                        state.replies.insert(key, reply);
                    }
                    Err(e) => log::debug!("Clean-up while speaking skipped a sentence: {e}"),
                }
            }
            let _ = done.send(true);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Heard;

    /// Only a new complete sentence counts; a revised text starts over.
    #[test]
    fn only_a_new_complete_sentence_counts() {
        let mut heard = Heard::default();
        assert!(!heard.has_new_sentence("We ship on"));
        assert!(heard.has_new_sentence("We ship on Monday. Bring"));
        assert!(!heard.has_new_sentence("We ship on Monday. Bring the"));
        assert!(heard.has_new_sentence("We ship on Monday. Bring the slides. And"));
        assert!(heard.has_new_sentence("We ship on Tuesday. Bring the slides. And"));
        assert!(!heard.has_new_sentence("We ship on Tuesday. Bring the slides. And the"));
    }
}
