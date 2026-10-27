# Notetaker backend contract

Everything the UI needs to build the Notetaker page, the pill's notetaker state, the
call prompt and the meeting reminder. All commands and events are exported to
`src/bindings.ts` by tauri-specta on the next debug run (`commands.notetakerStart(...)`,
`events.notetakerTranscript.listen(...)`). Commands that can fail return
`Result<T, string>`; the string is a user-readable reason.

All times are Unix milliseconds (UTC). Segment times are milliseconds from the start
of the meeting.

## Commands

| Command                             | Arguments                                                         | Returns                                                         | Notes                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| ----------------------------------- | ----------------------------------------------------------------- | --------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `notetaker_start`                   | `title: string \| null`                                           | `Meeting`                                                       | Starts recording mic + system audio. Title: given, else the calendar event happening now (or starting within 10 min), else "`<App>` call" when a call is detected, else "New note". A call detected at that moment is linked: the recording stops by itself when that call ends. Opens the notepad window, focused, on the new meeting (no need to call `notepad_open`). An auto-start shows the notepad without focus. Fails if already recording or another start is still opening the devices; `notetaker_status` keeps answering (with `recording: false`, `starting: true`) while a start waits on a device or permission prompt. A start that fails closes every stream it opened and removes the meeting; the error string says why. System audio that cannot start does not fail it: recording goes on with the microphone and `system.state = "unavailable"`. |
| `notetaker_stop`                    | none                                                              | `string` (meeting id)                                           | Stops, then runs the final transcript and notes in the background (watch `notetaker-processing`, `notetaker-note-ready`). During a start (`starting: true`) it cancels that start instead: `starting` turns false at once, and when the device or permission prompt the start waits on returns, the start closes its streams, removes its meeting, fails with "Recording was cancelled" and sends `notetaker-state` with `recording: false`, `meeting_id: null`. Until then a new `notetaker_start` fails with "A recording is already starting".                                                                                                                                                                                                                                                                                                                      |
| `notetaker_status`                  | none                                                              | `NotetakerStatus` (bindings: `Result<NotetakerStatus, string>`) | Poll for the recording screen (timer, stream health), or read once on mount. Never fails in practice.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| `notetaker_list`                    | `filter: NotetakerFilter \| null`                                 | `Meeting[]`                                                     | Newest first. Default limit 100. `query` matches the title, transcript, the user's thoughts and the words of the notes.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| `notetaker_get`                     | `id: string`                                                      | `MeetingDetail \| null`                                         | Meeting, transcript, the user's thoughts, notes.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| `notetaker_save_thoughts`           | `id, markdown`                                                    | `void`                                                          | The user's own notes ("My notes"). Save on edit (debounced). Allowed while recording. Fails for an unknown id.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| `notetaker_delete`                  | `id`                                                              | `void`                                                          | If it is the meeting being recorded, recording stops (nothing is processed) and it is deleted. If it is the meeting of a start still opening the devices, that start is cancelled as with `notetaker_stop`. Deletes audio too, and removes citations of it from all-meetings chats. Fails while its notes are being made.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| `notetaker_rename`                  | `id, title`                                                       | `void`                                                          | Marks the title as the user's (notes will no longer replace it, even if they are being written right now). Fails on an empty title.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    |
| `notetaker_rename_speaker`          | `meeting_id, speaker_key, name`                                   | `void`                                                          | Names one speaker (`speaker_key` from a segment) on every line of the meeting, then sends `notetaker-transcript` with `replace: true`. A voice-split speaker's voice is saved under that name (see Speakers). Fails on an empty name or a key the meeting doesn't have.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| `notetaker_saved_voices`            |                                                                   | `number`                                                        | How many people have a saved voice (see Speakers).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| `notetaker_forget_voices`           |                                                                   | `void`                                                          | Deletes every saved voice, in all meetings.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| `notetaker_move_to_folder`          | `id, folder: string \| null`                                      | `void`                                                          | `null` or empty removes it from its folder. Folders exist only through their meetings.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| `notetaker_folders`                 | none                                                              | `FolderInfo[]`                                                  | For the Folders tab.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| `notetaker_regenerate`              | `id`                                                              | `void`                                                          | Writes the notes again, keeping the transcript (after the AI model became available, or thoughts changed). For a meeting with `status: "failed"` the final transcript is made again too. Fails while the meeting is being recorded or processed.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| `notetaker_upcoming`                | none                                                              | `CalendarEvent[]`                                               | Today + tomorrow (local), sorted by start. Shows the macOS calendar prompt the first time. Empty when access is denied: check `notetaker_calendar_access`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| `notetaker_calendar_access`         | none                                                              | `CalendarAccess`                                                | Never prompts.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| `notetaker_request_calendar_access` | none                                                              | `CalendarAccess`                                                | Prompts if never asked, waits for the answer. For the "Connect calendar" step.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| `notetaker_ask`                     | `question, meeting_id: string \| null, thread_id: string \| null` | `AskAnswer`                                                     | `meeting_id` = ask about one meeting; `null` = all meetings. Pass the returned `thread_id` to continue the chat. Fails with the reason when the AI model is not available (built-in model not downloaded, or Ollama not running). Takes 5-60 s.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| `notetaker_chats`                   | `meeting_id: string \| null`                                      | `ChatThread[]`                                                  | "Earlier questions": threads for one meeting, or the all-meetings threads when `null`. Newest first.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| `notetaker_chat`                    | `thread_id`                                                       | `ChatMessage[]`                                                 | Messages of one thread, oldest first.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| `notetaker_get_settings`            | none                                                              | `NotetakerSettings`                                             |                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| `notetaker_set_settings`            | `settings: NotetakerSettings`                                     | `void`                                                          | Saves all fields. Detection and reminders pick changes up within 2 s. Fails when `max_recording_minutes` is outside 1-600 or `notify_before_seconds` is over 3600.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |

## Events

| Event name                 | Payload                                                                                          | When                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| -------------------------- | ------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `notetaker-state`          | `{ recording: boolean, meeting_id: string \| null }`                                             | Recording starts or stops (any reason: button, call ended, max length). Drives the pill's green ring. On a stop, `meeting_id` is the meeting that just stopped. A cancelled start sends `recording: false` with `meeting_id: null` once it has removed its meeting.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| `notetaker-call-detected`  | `{ app: string, bundle_id: string }`                                                             | Once per call, ~4 s after a call app (Zoom, Teams, Slack, FaceTime, Webex, Discord, or a browser: Chrome, Safari, Arc, Dia, Brave, Edge, Firefox, Chromium, Vivaldi, Opera, Zen) starts using the microphone. Browser helper processes count for their browser, also when Core Audio reports no or a helper bundle id (matched by the app the process runs inside). Show "Start Notetaker?" unless `auto_start` is on (then recording has already started; `notetaker-state` follows) or a recording runs. Auto-start is skipped while the notepad window has focus. A recording that runs without a call (started before joining) is linked to the call when this event is sent: its meeting gets `app`/`bundle_id`, a "New note" title becomes "`<App>` call", and it stops when that call ends. Re-fetch the meeting to show it. |
| `notetaker-call-ended`     | `{ app, bundle_id }`                                                                             | ~4 s after that app releases the microphone. For the call a recording is linked to, ~46 s (some apps release the microphone while muted); the recording stops at that moment.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| `notetaker-meeting-soon`   | `{ event: CalendarEvent, starts_in_seconds: number }`                                            | Once per event, `notify_before_seconds` before it starts (needs calendar access).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                   |
| `notetaker-live-text`      | `{ meeting_id, source: "mic" \| "system", speaker, start_ms, text, tentative, active: boolean }` | What one stream is hearing now, sent within ~0.3 s of speech starting (`text` and `tentative` empty: show a "hearing" bubble) and then as the words come (`text` = words the model has settled, `tentative` = words that may still change, show grey). `start_ms` identifies the utterance. `active: false` ends it: drop that bubble. If words were heard, its segment came just before in `notetaker-transcript` with the same `source` and `start_ms`. Plain Tauri event (not in `bindings.ts`): `listen<LiveText>("notetaker-live-text")`.                                                                                                                                                                                                                                                                                      |
| `notetaker-transcript`     | `{ meeting_id, segments: Segment[], replace: boolean }`                                          | `replace: false`: new live segments to append, one per utterance, when its speaker pauses (~0.7 s; utterances are cut at 30 s). `replace: true`: the full transcript; replace what is shown (after the final pass, and again after speakers are named).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| `notetaker-stream-changed` | `{ meeting_id, mic: StreamStatus, system: StreamStatus }`                                        | A stream's state or detail changed while recording (system audio silent → show the hint; a device lost → `unavailable` while it reconnects, then `capturing` again).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| `notetaker-processing`     | `{ meeting_id, stage: "speakers" \| "transcribing" \| "summarizing", progress: number }`         | `speakers`: telling the call voices apart, before the final pass (progress 0). During the final pass (progress 0..1) and while the notes are written (progress 0).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| `notetaker-note-ready`     | `{ meeting_id, summary_status, summary_error: string \| null }`                                  | Processing finished. Re-fetch with `notetaker_get`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |

## Types

```ts
type Meeting = {
  id: string;
  title: string;
  title_auto: boolean;
  start_ms: number;
  end_ms: number | null;
  app: string | null;
  bundle_id: string | null; // call app it was linked to
  calendar_event_id: string | null;
  attendees: Attendee[];
  folder: string | null;
  status: "recording" | "processing" | "ready" | "failed";
  summary_status: "pending" | "ready" | "unavailable" | "failed";
  summary_error: string | null; // show when unavailable/failed
  mic_offset_ms: number | null;
  system_offset_ms: number | null;
};
type Attendee = { name: string | null; email: string | null; is_me: boolean };
type Segment = {
  source: "mic" | "system";
  speaker: string; // "Me", "Them", "Speaker 1", "Speaker 2", … or a name
  speaker_key: string; // "me", "them" (call audio not split), "s1", "s2", …; fixed, every line of one key shares one label
  start_ms: number;
  end_ms: number;
  text: string;
};
type MeetingDetail = {
  meeting: Meeting;
  segments: Segment[];
  thoughts: string;
  summary: NoteSummary | null;
};
type NoteSummary = {
  title: string | null; // suggested; already applied when title_auto
  overview: string; // 2-4 sentences, in the meeting's language
  sections: { heading: string; bullets: string[] }[];
  key_points: string[];
  decisions: string[];
  action_items: { text: string; owner: string | null; due: string | null }[];
};
type NotetakerFilter = {
  query: string | null;
  folder: string | null;
  after_ms: number | null;
  before_ms: number | null;
  limit: number | null;
  offset: number | null;
};
type FolderInfo = { name: string; count: number };
type CalendarEvent = {
  id: string; // unique per occurrence
  title: string;
  start_ms: number;
  end_ms: number;
  all_day: boolean;
  attendees: Attendee[];
  organizer: Attendee | null;
  meeting_url: string | null; // Zoom/Meet/Teams/Webex/Slack/FaceTime link from URL, location or notes
  location: string | null;
  calendar: string | null;
};
type CalendarAccess =
  | "not_determined"
  | "denied"
  | "restricted"
  | "write_only"
  | "full_access"
  | "unsupported";
type StreamStatus = {
  state: "off" | "starting" | "capturing" | "silent" | "unavailable";
  detail: string | null;
};
type NotetakerStatus = {
  recording: boolean;
  starting: boolean; // a start is opening the devices (macOS may be showing the
  // System Audio Recording prompt); recording is still false.
  // mic/system are then "starting"
  meeting_id: string | null;
  title: string | null;
  started_ms: number | null;
  elapsed_ms: number;
  mic: StreamStatus;
  system: StreamStatus;
  active_calls: { app: string; bundle_id: string }[];
  processing: string[]; // meeting ids being transcribed / summarized
  calendar_access: CalendarAccess;
};
type Citation = {
  marker: number;
  meeting_id: string;
  meeting_title: string;
  start_ms: number;
  quote: string;
};
type ChatMessage = {
  id: number;
  thread_id: string;
  meeting_id: string | null;
  role: "user" | "assistant";
  content: string;
  citations: Citation[];
  created_ms: number;
};
type ChatThread = {
  thread_id: string;
  meeting_id: string | null;
  started_ms: number;
  updated_ms: number;
  first_question: string;
};
type AskAnswer = {
  thread_id: string;
  question: ChatMessage;
  answer: ChatMessage;
};
```

The answer text is markdown with `[n]` markers; `citations[].marker` is the `n`.
`start_ms` of a citation is the transcript time to jump to (0 for passages taken from
the notes or thoughts).

## Settings (`NotetakerSettings`, stored under `notetaker` in the app settings)

| Field                           | Default                  | Meaning                                              |
| ------------------------------- | ------------------------ | ---------------------------------------------------- |
| `detect_calls: boolean`         | `true`                   | Watch for calls in other apps                        |
| `auto_start: boolean`           | `false`                  | Start recording when a call is detected              |
| `notify_before_seconds: number` | `60` (0 = off)           | Seconds before a calendar meeting to show a reminder |
| `max_recording_minutes: number` | `120`                    | Recording stops after this many minutes              |
| `ollama_url: string`            | `http://127.0.0.1:11434` | Ollama server, when the engine is Ollama             |
| `ollama_model: string`          | `qwen3:8b`               | Model used on that server                            |

The backend does not show the next meeting in the menu bar.

Notes and Ask use the same engine as clean-up (`cleanup_engine`, see
`textflow/CONTRACT.md`): the built-in engine (with a 16384-token context and
JSON output) or Ollama at `ollama_url` with `ollama_model`.

## Speakers

Before the final pass, the call audio (system.wav) is split by voice with
speakrs (pyannote community-1 pipeline, CPU, about half the cores; about 1-2
minutes and 0.5-1 GB for an hour). It is skipped when the live transcript has no
call lines, and for call audio over 3 hours. Its models (33 MB) are downloaded on
first use into `<app data>/models/speakers/`, pinned to one revision and
hash-checked. Pieces of call audio are also cut where the speaker changes (turns
under 1 s don't cut), and each line goes to the speaker heard most during it.
Speakers are numbered by first appearance (`s1` = "Speaker 1"). A voice with
under 25 s of speech joins the voice it sounds most like if they are at least
0.5 alike, else its lines stay "Them". If anything fails (offline on first use,
silent call audio), the call stays one "Them" (`speaker_key` "them"); the
meeting never fails because of it. The microphone is always "Me". Lines from
before speaker detection keep one key per old name (`n:<name>`).

Names, in order, only for speakers still on their default label:

1. A saved voice: the speaker's average voice matches a voice profile at cosine
   0.8 or more, and beats the next profile by 0.05.
2. The calendar: exactly one other invitee and one voice heard.
3. The notes model: per speaker, only when the name is said in at least two of
   the lines it points to, each said by that speaker (an introduction) or said
   by someone else right before that speaker talks. Never the user's own name or
   email (calendar, macOS full name, emails in the dictionary), never a name
   another speaker claims as strongly. Applied only to speakers still on their
   default label when the notes are saved.
4. The user: `notetaker_rename_speaker` (refused while the meeting is being
   processed). Each meeting's voices are kept in `meeting_voices`; naming one
   marks it, and a voice profile is the average of the marked voices under each
   current name. Renaming again moves the voice to the new name; deleting a
   meeting removes its voices. `notetaker_saved_voices` counts the people with a
   profile and `notetaker_forget_voices` deletes every voice (Settings > Privacy).

## Files

`<app data>/notetaker/notetaker.db` (SQLite: meetings, segments, my_thoughts,
summaries, chat_messages, meeting_voices, voice_profiles) and `<app data>/notetaker/<meeting_id>/mic.wav` +
`system.wav` (16 kHz mono 16-bit). Both files stay on the wall clock: a gap (device
unplugged, output device switched) is filled with silence. Meetings left
`recording`/`processing` by a crash are finished from these files at the next
launch; one whose processing was cut off twice is set to `failed` with a reason
instead (Regenerate retries it).

## Stream recovery while recording

- Microphone: when it fails (unplugged), it is reopened every 5 s, falling back to
  the system default microphone. It uses the clamshell microphone setting when the
  lid is closed, like dictation.
- System audio: when its audio stops for 2 s, or the default output device changes,
  the capture is rebuilt on the current output device.
- When live transcription falls more than 2 minutes behind (slow model, long
  dictation), the oldest live audio is skipped (the utterance in progress ends
  there). The final pass still covers it.

## Live transcript

Each stream (microphone, system audio) is cut into utterances by the VAD. With a
model that can stream (Parakeet unified), each utterance is streamed to a worker
process of the stream's own, so the words show while they are spoken and
dictation never waits for the notetaker (or the other way round). The
microphone's worker loads when recording starts, system audio's once the call
plays a sound; each costs about the model's size in memory and stops after 5
minutes without speech. With a model that can't stream, each utterance is
decoded when it ends, on the dictation engine, after any dictation in progress.
The final pass after the meeting replaces the live transcript.

## Permissions the UI should explain

- Microphone: already asked for by dictation.
- System Audio Recording Only (macOS 14.4+): asked by macOS the first time
  `notetaker_start` runs. A denial is silent: the stream gets only zeros. After 30 s
  of zeros while a call app is detected it shows up as `system.state = "silent"`
  with a "nothing heard from the call yet" hint in `detail` (the other side may
  simply not have joined). The microphone shows `silent` after 10 s of zeros.
- Calendars: asked by `notetaker_upcoming` / `notetaker_request_calendar_access`.
