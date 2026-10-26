import type { Note } from "@/bindings";
import type { MockHandler } from "../mockTauri";

// ?scratchpad=sample|note starts with three notes; plain ?page=scratchpad starts empty.
const mode = new URLSearchParams(window.location.search).get("scratchpad");
const HOUR = 3_600_000;
const now = Date.now();

let notes: Note[] =
  mode === "sample" || mode === "note"
    ? [
        {
          id: "sample-1",
          title: "Weekend errands",
          body: "Pick up the bike from the repair shop.\nBuy coffee beans and oat milk.\nReturn the library books before Saturday.",
          updated_at: now - 2 * HOUR,
        },
        {
          id: "sample-2",
          title: "Talk outline",
          body: "Open with the problem in one sentence. Show the before and after. Keep the demo under three minutes and leave time for questions.",
          updated_at: now - 26 * HOUR,
        },
        {
          id: "sample-3",
          title: "",
          body: "Idea: a small weekly note that lists what shipped, what slipped and what we learned.",
          updated_at: now - 6 * 24 * HOUR,
        },
      ]
    : [];

export const handlers: Record<string, MockHandler> = {
  list_notes: () => [...notes].sort((a, b) => b.updated_at - a.updated_at),
  get_note: ({ id }) => notes.find((n) => n.id === id) ?? null,
  save_note: ({ id, title, body }) => {
    const noteId = typeof id === "string" && id ? id : `note-${Date.now()}`;
    const note = { id: noteId, title: String(title), body: String(body), updated_at: Date.now() };
    notes = [note, ...notes.filter((n) => n.id !== noteId)];
    return noteId;
  },
  delete_note: ({ id }) => {
    notes = notes.filter((n) => n.id !== id);
    return null;
  },
};
