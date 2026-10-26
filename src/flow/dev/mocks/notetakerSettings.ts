import type { CalendarAccess, CalendarSource, NotetakerSettings } from "@/bindings";
import type { MockHandler } from "../mockTauri";

// Settings → Notetaker in the preview. `?calendar=denied` (etc.) picks the
// calendar access state; the default is "not_determined".
const params = new URLSearchParams(window.location.search);

let settings: NotetakerSettings = {
  detect_calls: true,
  auto_start: false,
  notify_before_seconds: 60,
  max_recording_minutes: 120,
  ollama_url: "http://127.0.0.1:11434",
  ollama_model: "qwen3:8b",
};

let calendar = (params.get("calendar") ?? "not_determined") as CalendarAccess;

// `?calendarSources=none` shows the "no accounts" state of the connect modal.
const SAMPLE_SOURCES: CalendarSource[] = [
  { id: "g", title: "asha.rao@example.com", kind: "google", calendars: ["asha.rao@example.com", "Holidays", "Team"] },
  { id: "x", title: "Exchange", kind: "exchange", calendars: ["Calendar"] },
  { id: "i", title: "iCloud", kind: "icloud", calendars: ["Family", "Home"] },
];

export const handlers: Record<string, MockHandler> = {
  notetaker_get_settings: () => settings,
  notetaker_set_settings: ({ settings: next }) => {
    const s = next as NotetakerSettings;
    if (s.max_recording_minutes < 1 || s.max_recording_minutes > 600)
      throw "Maximum recording length must be between 1 and 600 minutes";
    if (s.notify_before_seconds > 3600)
      throw "Reminders can be at most an hour before a meeting";
    settings = s;
    return null;
  },
  notetaker_calendar_access: () => calendar,
  notetaker_request_calendar_access: () => {
    if (calendar === "not_determined") calendar = "full_access";
    return calendar;
  },
  notetaker_calendar_sources: () =>
    calendar !== "full_access" || params.get("calendarSources") === "none"
      ? [{ id: "local", title: "On My Mac", kind: "local", calendars: ["Calendar"] }]
      : SAMPLE_SOURCES,
};
