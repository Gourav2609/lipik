import { create } from "zustand";
import { commands, type StyleKind, type StyleSettings } from "@/bindings";
import { errorText, onTextflowChanged, unwrap } from "../textflow";
import {
  CONTEXTS,
  type CleanupLevel,
  type ContextId,
  type StyleId,
} from "./data";

// The backend has no cleanup level, so the page keeps it.
interface Saved {
  cleanupLevel: CleanupLevel;
}

interface StyleState extends Saved {
  styles: Record<ContextId, StyleId>;
  autoCleanup: boolean;
  setStyle: (context: ContextId, style: StyleId) => void;
  setAutoCleanup: (enabled: boolean) => void;
  setCleanupLevel: (level: CleanupLevel) => void;
}

const KEY = "lipik.style.v2";

const DEFAULTS: Saved = { cleanupLevel: "medium" };

const load = (): Saved => {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return DEFAULTS;
    const saved = JSON.parse(raw) as Partial<Saved>;
    return { cleanupLevel: saved.cleanupLevel ?? DEFAULTS.cleanupLevel };
  } catch {
    return DEFAULTS;
  }
};

const save = (s: Saved) => {
  try {
    localStorage.setItem(KEY, JSON.stringify(s));
  } catch {
    // storage unavailable (private window): choices last for this session only
  }
};

const STYLE_IDS: StyleId[] = ["formal", "casual", "very-casual"];

// Styles the page kept under KEY before the backend stored them; sent once, then dropped from KEY.
const readOldStyles = (): Partial<Record<ContextId, StyleId>> => {
  try {
    const saved = JSON.parse(localStorage.getItem(KEY) ?? "{}") as {
      styles?: Record<string, unknown>;
    };
    const out: Partial<Record<ContextId, StyleId>> = {};
    for (const c of CONTEXTS) {
      const v = saved.styles?.[c.id];
      if (STYLE_IDS.includes(v as StyleId)) out[c.id] = v as StyleId;
    }
    return out;
  } catch {
    return {};
  }
};

const toStyleId = (kind: StyleKind): StyleId =>
  kind === "very_casual" ? "very-casual" : kind;
const toKind = (id: StyleId): StyleKind =>
  id === "very-casual" ? "very_casual" : id;

const fromBackend = (s: StyleSettings) => ({
  styles: {
    personal: toStyleId(s.personal),
    work: toStyleId(s.work),
    email: toStyleId(s.email),
    other: toStyleId(s.other),
  },
  autoCleanup: s.auto_cleanup,
});

const preview =
  new URLSearchParams(window.location.search).get("flowPreview") === "1";

const initial: Saved = preview ? DEFAULTS : load();

export const useStyleStore = create<StyleState>((set, get) => {
  const update = (patch: Partial<Saved>) => {
    set(patch);
    save({ cleanupLevel: get().cleanupLevel });
  };
  let lastCall = 0;
  const apply = async (call: ReturnType<typeof commands.setStyle>) => {
    const id = ++lastCall;
    try {
      const saved = unwrap(await call);
      if (id === lastCall) set(fromBackend(saved));
    } catch (err) {
      console.error("Failed to save style:", errorText(err));
      void reload();
    }
  };
  const reload = async () => {
    try {
      set(fromBackend(await commands.getStyleSettings()));
    } catch (err) {
      console.error("Failed to load style settings:", err);
    }
  };
  const migrateOld = async () => {
    const old = readOldStyles();
    for (const [context, style] of Object.entries(old) as [
      ContextId,
      StyleId,
    ][]) {
      unwrap(await commands.setStyle(context, toKind(style)));
    }
    save({ cleanupLevel: get().cleanupLevel });
  };
  if (!preview)
    void migrateOld()
      .catch((err) =>
        console.error("Failed to move styles to the backend:", errorText(err)),
      )
      .then(reload);
  onTextflowChanged("style", () => void reload());
  return {
    ...initial,
    styles: {
      personal: "formal",
      work: "formal",
      email: "formal",
      other: "formal",
    },
    autoCleanup: true,
    setStyle: (context, style) => {
      set({ styles: { ...get().styles, [context]: style } });
      void apply(commands.setStyle(context, toKind(style)));
    },
    setAutoCleanup: (autoCleanup) => {
      set({ autoCleanup });
      void apply(commands.setAutoCleanup(autoCleanup));
    },
    setCleanupLevel: (cleanupLevel) => update({ cleanupLevel }),
  };
});
