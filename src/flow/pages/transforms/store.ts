import { useSyncExternalStore } from "react";
import { toast } from "sonner";
import { commands, type Result, type TransformSettings } from "@/bindings";
import { errorText, onTextflowChanged, unwrap } from "../textflow";

export interface Transform {
  id: string;
  name: string;
  description: string;
  prompt: string;
  /** 1..9 for ⌥1..⌥9, or null. */
  shortcut: number | null;
  builtIn: boolean;
}

export interface TransformsState {
  optIn: boolean;
  autoApply: boolean;
  autoApplyId: string;
  transforms: Transform[];
}

export type TransformDraft = Pick<Transform, "name" | "description" | "prompt">;

const BUILT_IN_IDS = new Set(["polish", "prompt_engineer"]);
// What the page kept here before the backend stored transforms.
const OLD_KEY = "lipik.transforms.v1";

let state: TransformsState = {
  optIn: false,
  autoApply: false,
  autoApplyId: "",
  transforms: [],
};
const listeners = new Set<() => void>();
const notify = () => listeners.forEach((l) => l());

const commit = (s: TransformSettings) => {
  const transforms = s.items.map((t) => ({
    id: t.id,
    name: t.name,
    description: t.description,
    prompt: t.prompt,
    shortcut: t.shortcut_index,
    builtIn: BUILT_IN_IDS.has(t.id),
  }));
  const chosen = s.auto_apply.transform_id;
  state = {
    optIn: s.opt_in,
    autoApply: s.auto_apply.enabled,
    autoApplyId:
      chosen && transforms.some((t) => t.id === chosen)
        ? chosen
        : (transforms[0]?.id ?? ""),
    transforms,
  };
  notify();
};

const run = async (call: Promise<Result<TransformSettings, string>>) => {
  try {
    commit(unwrap(await call));
  } catch (err) {
    toast(errorText(err));
    reload();
  }
};

const reload = () => {
  commands
    .getTransforms()
    .then(commit)
    .catch((err) => console.error("Failed to load transforms:", err));
};

// The page's old ids for the built-ins, before the backend stored them.
const OLD_BUILT_IN_IDS: Record<string, string> = {
  polish: "polish",
  "prompt-engineer": "prompt_engineer",
};

type OldTransform = Partial<
  Pick<Transform, "id" | "name" | "description" | "prompt">
>;

const migrateOld = async () => {
  let raw: string | null = null;
  try {
    raw = localStorage.getItem(OLD_KEY);
  } catch {
    return;
  }
  if (!raw) return;
  try {
    const old = JSON.parse(raw) as {
      optIn?: boolean;
      autoApply?: boolean;
      autoApplyId?: string;
      transforms?: OldTransform[];
    };
    let current = await commands.getTransforms();
    const newIds = new Map<string, string>();
    for (const t of old.transforms ?? []) {
      if (!t.id) continue;
      const builtInId = OLD_BUILT_IN_IDS[t.id];
      if (builtInId) {
        const base = current.items.find((i) => i.id === builtInId);
        if (
          !base ||
          (t.name === undefined &&
            t.description === undefined &&
            t.prompt === undefined)
        )
          continue;
        current = unwrap(
          await commands.saveTransform(
            base.id,
            t.name ?? base.name,
            t.description ?? base.description,
            t.prompt ?? base.prompt,
            base.shortcut_index,
          ),
        );
        newIds.set(t.id, base.id);
      } else if (t.id.startsWith("custom-") && t.name && t.prompt) {
        const before = new Set(current.items.map((i) => i.id));
        current = unwrap(
          await commands.saveTransform(
            null,
            t.name,
            t.description ?? "",
            t.prompt,
            null,
          ),
        );
        const added = current.items.find((i) => !before.has(i.id));
        if (added) newIds.set(t.id, added.id);
      }
    }
    unwrap(await commands.setTransformsOptIn(old.optIn !== false));
    const autoApplyId =
      (old.autoApplyId &&
        (OLD_BUILT_IN_IDS[old.autoApplyId] ?? newIds.get(old.autoApplyId))) ||
      null;
    unwrap(await commands.setAutoApply(old.autoApply === true, autoApplyId));
    localStorage.removeItem(OLD_KEY);
  } catch (err) {
    console.error("Failed to move transforms to the backend:", err);
  }
};

if (typeof window !== "undefined") {
  void migrateOld().then(reload);
  onTextflowChanged("transforms", reload);
}

const subscribe = (l: () => void) => {
  listeners.add(l);
  return () => listeners.delete(l);
};

export const useTransforms = () => useSyncExternalStore(subscribe, () => state);

export const transformsActions = {
  setOptIn: (optIn: boolean) => {
    state = { ...state, optIn };
    notify();
    void run(commands.setTransformsOptIn(optIn));
  },
  setAutoApply: (autoApply: boolean) => {
    state = { ...state, autoApply };
    notify();
    void run(commands.setAutoApply(autoApply, state.autoApplyId || null));
  },
  setAutoApplyId: (autoApplyId: string) => {
    state = { ...state, autoApplyId };
    notify();
    void run(commands.setAutoApply(state.autoApply, autoApplyId));
  },
  add: (draft: TransformDraft) =>
    void run(
      commands.saveTransform(
        null,
        draft.name,
        draft.description,
        draft.prompt,
        null,
      ),
    ),
  update: (id: string, draft: TransformDraft) => {
    const shortcut =
      state.transforms.find((t) => t.id === id)?.shortcut ?? null;
    void run(
      commands.saveTransform(
        id,
        draft.name,
        draft.description,
        draft.prompt,
        shortcut,
      ),
    );
  },
  remove: (id: string) => {
    if (BUILT_IN_IDS.has(id)) return;
    void run(commands.deleteTransform(id));
  },
  resetTransforms: () => void run(commands.resetTransforms()),
};
