import { create } from "zustand";
import { toast } from "sonner";
import { commands } from "@/bindings";
import { isFlowPreview } from "../../dev/preview";
import { errorText, unwrap } from "../textflow";

export interface Note {
  id: string;
  title: string;
  body: string;
  updatedAt: number;
}

interface ScratchpadState {
  notes: Note[];
  loaded: boolean;
  addToBar: boolean;
  createNote: () => string;
  updateNote: (
    id: string,
    patch: Partial<Pick<Note, "title" | "body">>,
  ) => void;
  deleteNote: (id: string) => void;
  dropBlank: () => void;
  setAddToBar: (on: boolean) => void;
  reload: () => void;
}

// Before notes moved to the backend this key held them too; now only `addToBar`.
const KEY = "lipik.scratchpad.v1";
const SAVE_DELAY_MS = 400;

const params = new URLSearchParams(window.location.search);
const previewMode = isFlowPreview ? params.get("scratchpad") : null;

const readLocal = (): { notes: Note[]; addToBar: boolean } => {
  try {
    const saved = JSON.parse(localStorage.getItem(KEY) ?? "{}") as {
      notes?: Note[];
      addToBar?: boolean;
    };
    return {
      notes: Array.isArray(saved.notes) ? saved.notes : [],
      addToBar: saved.addToBar === true,
    };
  } catch {
    return { notes: [], addToBar: false };
  }
};

const saveAddToBar = (addToBar: boolean) => {
  try {
    localStorage.setItem(KEY, JSON.stringify({ addToBar }));
  } catch {
    /* storage unavailable: the choice lasts for this session */
  }
};

const isBlank = (n: Pick<Note, "title" | "body">) =>
  !n.title.trim() && !n.body.trim();

const newId = () =>
  typeof crypto !== "undefined" && "randomUUID" in crypto
    ? crypto.randomUUID()
    : `${Date.now()}-${Math.random().toString(36).slice(2)}`;

const fetchNotes = async (): Promise<Note[]> =>
  unwrap(await commands.listNotes()).map((n) => ({
    id: n.id,
    title: n.title,
    body: n.body,
    updatedAt: n.updated_at,
  }));

const timers = new Map<string, ReturnType<typeof setTimeout>>();
// Notes the backend has a row for, so a note emptied later is deleted there.
const stored = new Set<string>();
// One queue per note, so an older body can never be written after a newer one.
const queues = new Map<string, Promise<void>>();

const enqueue = (id: string, job: () => Promise<void>) => {
  const next = (queues.get(id) ?? Promise.resolve()).then(job).catch((err) => {
    console.error("Failed to save note:", err);
    toast(`Couldn't save note: ${errorText(err)}`);
  });
  queues.set(id, next);
  void next.then(() => {
    if (queues.get(id) === next) queues.delete(id);
  });
  return next;
};

const persist = (note: Note) => {
  timers.delete(note.id);
  return enqueue(note.id, async () => {
    if (!isBlank(note)) {
      unwrap(await commands.saveNote(note.id, note.title, note.body));
      stored.add(note.id);
    } else if (stored.delete(note.id)) {
      unwrap(await commands.deleteNote(note.id));
    }
  });
};

const schedule = (note: Note) => {
  clearTimeout(timers.get(note.id));
  timers.set(
    note.id,
    setTimeout(() => void persist(note), SAVE_DELAY_MS),
  );
};

const flushAll = (notes: Note[]) =>
  Promise.all(
    [...timers.keys()].map((id) => {
      clearTimeout(timers.get(id));
      timers.delete(id);
      const note = notes.find((n) => n.id === id);
      return note ? persist(note) : undefined;
    }),
  );

const migrateLocalNotes = async () => {
  const { notes, addToBar } = readLocal();
  if (!notes.length) return;
  const oldestFirst = [...notes].sort(
    (a, b) => (a.updatedAt ?? 0) - (b.updatedAt ?? 0),
  );
  for (const n of oldestFirst) {
    if (typeof n.id !== "string" || isBlank(n)) continue;
    unwrap(await commands.saveNote(n.id, n.title ?? "", n.body ?? ""));
  }
  saveAddToBar(addToBar);
};

export const openNewestOnLoad = previewMode === "note";

export const useScratchpad = create<ScratchpadState>((set, get) => {
  const load = async () => {
    try {
      const notes = await fetchNotes();
      notes.forEach((n) => stored.add(n.id));
      const editing = get().notes.filter((n) => timers.has(n.id) || isBlank(n));
      set({
        loaded: true,
        notes: [
          ...editing,
          ...notes.filter((n) => !editing.some((e) => e.id === n.id)),
        ],
      });
    } catch (err) {
      console.error("Failed to load notes:", err);
      set({ loaded: true });
    }
  };
  void migrateLocalNotes()
    .catch((err) => console.error("Failed to move notes to the backend:", err))
    .then(load);
  window.addEventListener("beforeunload", () => void flushAll(get().notes));
  document.addEventListener("visibilitychange", () => {
    if (document.visibilityState === "hidden") void flushAll(get().notes);
  });

  return {
    notes: [],
    loaded: false,
    addToBar: readLocal().addToBar,
    createNote: () => {
      const note: Note = {
        id: newId(),
        title: "",
        body: "",
        updatedAt: Date.now(),
      };
      const notes = get().notes;
      void flushAll(notes);
      set({ notes: [note, ...notes.filter((n) => !isBlank(n))] });
      return note.id;
    },
    updateNote: (id, patch) => {
      const notes = get().notes.map((n) =>
        n.id === id ? { ...n, ...patch, updatedAt: Date.now() } : n,
      );
      set({ notes });
      const note = notes.find((n) => n.id === id);
      if (note) schedule(note);
    },
    deleteNote: (id) => {
      clearTimeout(timers.get(id));
      timers.delete(id);
      stored.delete(id);
      set({ notes: get().notes.filter((n) => n.id !== id) });
      void enqueue(id, async () => {
        unwrap(await commands.deleteNote(id));
      });
    },
    dropBlank: () => {
      const notes = get().notes;
      void flushAll(notes);
      if (notes.some(isBlank)) set({ notes: notes.filter((n) => !isBlank(n)) });
    },
    setAddToBar: (addToBar) => {
      set({ addToBar });
      saveAddToBar(addToBar);
    },
    reload: () => {
      void flushAll(get().notes).then(load);
    },
  };
});

export const noteTitle = (note: Note) =>
  note.title.trim() || note.body.trim().split("\n")[0] || "Untitled";

export const notePreview = (note: Note) => {
  const body = note.body.trim();
  const rest = note.title.trim() ? body : body.split("\n").slice(1).join(" ");
  return rest.replace(/\s+/g, " ");
};

export const formatUpdated = (ts: number) => {
  const d = new Date(ts);
  const now = new Date();
  const sameDay = d.toDateString() === now.toDateString();
  if (sameDay) {
    return d.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
  }
  const sameYear = d.getFullYear() === now.getFullYear();
  return d.toLocaleDateString([], {
    month: "short",
    day: "numeric",
    ...(sameYear ? {} : { year: "numeric" }),
  });
};
