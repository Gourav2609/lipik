import { openUrl } from "@tauri-apps/plugin-opener";
import { commands } from "@/bindings";

export const openMainPage = (page: string) =>
  commands.openMainPage(page).catch(() => {});

export const hidePillFor = (minutes: number) =>
  commands.pillHideFor(minutes).catch(() => {});

export const pasteLastTranscript = () =>
  commands.pasteLastTranscript().catch(() => {});

/** Resolves false when the stop failed, so no `notetaker-state` event will follow. */
export const stopNotetaker = async () => {
  const r = await commands.notetakerStop().catch(() => null);
  return r?.status === "ok";
};

/** Brings the notepad forward; the main window's Notetaker page when there is no notepad. */
export const openNotepad = async (meetingId: string | null) => {
  const r = await commands.notepadOpen(meetingId).catch(() => null);
  if (r?.status !== "ok") openMainPage("notetaker");
};

/** The backend opens the notepad itself when the recording starts. */
export const startNotetaker = async (title: string | null) => {
  const r = await commands.notetakerStart(title).catch((e) => {
    console.warn("Could not start meeting notes:", e);
    return null;
  });
  if (r?.status === "error") console.warn("Could not start meeting notes:", r.error);
  return r?.status === "ok";
};

export const notetakerStatus = () =>
  commands
    .notetakerStatus()
    .then((r) => (r.status === "ok" ? r.data : null))
    .catch(() => null);

export const openLink = (url: string) => openUrl(url).catch(() => {});

/** Resolves false when the meeting cannot be deleted yet (its notes are being made). */
export const deleteMeeting = async (id: string) => {
  const r = await commands.notetakerDelete(id).catch(() => null);
  return r?.status === "ok";
};

const FEW_WORDS = 15;
const SHORT_MS = 60_000;

/** "Started by mistake?": a short meeting with only a few live words. */
export const startedByMistake = async (id: string) => {
  const r = await commands.notetakerGet(id).catch(() => null);
  const detail = r?.status === "ok" ? r.data : null;
  if (!detail) return false;
  const { start_ms, end_ms } = detail.meeting;
  const words = detail.segments
    .map((s) => s.text.split(/\s+/).filter(Boolean).length)
    .reduce((a, b) => a + b, 0);
  return words < FEW_WORDS && (end_ms ?? Date.now()) - start_ms < SHORT_MS;
};

export const calendarConnected = () =>
  commands
    .notetakerCalendarAccess()
    .then((a) => a === "full_access")
    .catch(() => false);

export const pillHiddenUntil = () =>
  commands.pillHiddenUntil().catch(() => null);

export type Meeting = { id: string; title: string; at: string };

const endOfToday = () => {
  const d = new Date();
  d.setHours(24, 0, 0, 0);
  return d.getTime();
};

/** Today's meetings that have not ended yet (the command also returns tomorrow's). */
export const upcomingMeetings = async (): Promise<Meeting[]> => {
  const r = await commands.notetakerUpcoming().catch(() => null);
  if (r?.status !== "ok") return [];
  const now = Date.now();
  const end = endOfToday();
  return r.data
    .filter((e) => !e.all_day && e.end_ms > now && e.start_ms < end)
    .map((e) => ({
      id: e.id,
      title: e.title,
      at: new Date(e.start_ms).toLocaleTimeString([], {
        hour: "numeric",
        minute: "2-digit",
      }),
    }));
};
