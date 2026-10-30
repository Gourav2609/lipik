import { create } from "zustand";
import { locale } from "@tauri-apps/plugin-os";
import type { ModelInfo } from "@/bindings";
import { useModelStore } from "@/stores/modelStore";
import { isFlowPreview } from "../dev/preview";
import {
  baseLanguage,
  DEFAULT_MODEL_ID,
  EUROPEAN_MODEL_ID,
  modelForLanguage,
  WORLD_MODEL_ID,
} from "./speechLanguage";

export { DEFAULT_MODEL_ID } from "./speechLanguage";

const CHOICE_KEY = "lipik.onboarding.speechModel";

const readChoice = () => {
  try {
    return localStorage.getItem(CHOICE_KEY);
  } catch {
    return null;
  }
};

const previewLanguage = () =>
  new URLSearchParams(window.location.search).get("lang") ?? "en-US";

/** The Mac's language (null until known) and a model the person picked instead. */
const useChoice = create<{ language: string | null; picked: string | null }>(
  () => ({
    language: isFlowPreview ? previewLanguage() : null,
    picked: isFlowPreview ? null : readChoice(),
  }),
);

let languageAsked = false;
const askLanguage = () => {
  if (languageAsked || isFlowPreview) return;
  languageAsked = true;
  locale()
    .then((tag) => useChoice.setState({ language: tag ?? "en" }))
    .catch(() => useChoice.setState({ language: "en" }));
};

const chosenId = (models: ModelInfo[]) => {
  const { language, picked } = useChoice.getState();
  if (picked && models.some((m) => m.id === picked)) return picked;
  return language === null ? null : modelForLanguage(language, models);
};

export type ModelSetupStatus =
  | "waiting"
  | "downloading"
  | "verifying"
  | "loading"
  | "ready"
  | "failed";

export interface ModelSetup {
  status: ModelSetupStatus;
  percent: number;
  name: string;
  /** Why the download failed, when it did. */
  reason?: string | null;
}

const pickDefault = (models: ModelInfo[]): ModelInfo | undefined => {
  const id = chosenId(models);
  if (id === null) return undefined;
  return (
    models.find((m) => m.id === id) ??
    models.find((m) => m.id === DEFAULT_MODEL_ID) ??
    models.find((m) => m.is_recommended) ??
    models[0]
  );
};

const useFailed = create<{ failed: boolean; reason: string | null }>(() => ({
  failed: false,
  reason: null,
}));
const setFailed = (failed: boolean, reason: string | null = null) =>
  useFailed.setState({ failed, reason });

let started = false;
let selecting = false;
// The model whose download was asked for, until the call returns: the store
// only lists it as downloading later, and every store change runs `step`
// again.
let requesting: string | null = null;

const step = async () => {
  const store = useModelStore.getState();
  const model = pickDefault(store.models);
  if (!model || selecting) return;
  const id = model.id;
  if (store.currentModel === id && model.is_downloaded) {
    unsubscribe?.();
    return;
  }
  if (model.is_downloaded) {
    if (id in store.downloadingModels || id in store.verifyingModels) return;
    selecting = true;
    const ok = await store.selectModel(id);
    selecting = false;
    if (ok) unsubscribe?.();
    else setFailed(true);
    return;
  }
  if (id in store.downloadingModels || id in store.verifyingModels) return;
  if (useFailed.getState().failed || requesting === id) return;
  requesting = id;
  const ok = await store.downloadModel(id);
  if (requesting === id) requesting = null;
  if (!ok && pickDefault(useModelStore.getState().models)?.id === id)
    setFailed(true, useModelStore.getState().error);
};

let unsubscribe: (() => void) | undefined;

/** Downloads and selects the default model once, without any UI blocking on it. */
export const startDefaultModelSetup = () => {
  if (started || isFlowPreview) return;
  started = true;
  askLanguage();
  const unsubscribeModels = useModelStore.subscribe(() => {
    step();
  });
  const unsubscribeChoice = useChoice.subscribe(() => {
    step();
  });
  unsubscribe = () => {
    unsubscribeModels();
    unsubscribeChoice();
  };
  step();
};

/** Uses another speech model for setup, e.g. English on a French Mac. */
export const chooseSpeechModel = async (id: string) => {
  const store = useModelStore.getState();
  const before = pickDefault(store.models)?.id;
  if (before === id) return;
  try {
    localStorage.setItem(CHOICE_KEY, id);
  } catch {
    /* private mode: the choice lasts for this launch */
  }
  setFailed(false);
  useChoice.setState({ picked: id });
  if (before && before in store.downloadingModels)
    await store.cancelDownload(before);
};

/** Restarts a failed download (e.g. after the network came back). */
export const retryDefaultModelSetup = () => {
  setFailed(false);
  step();
};

/** Whether the speech model no longer needs the connection to itself. */
const speechSettled = () => {
  const { models, currentModel } = useModelStore.getState();
  const model = pickDefault(models);
  if (useFailed.getState().failed) return true;
  if (!model) return false;
  return model.is_downloaded || (!!currentModel && currentModel !== model.id);
};

const useQueued = create<{ queued: boolean }>(() => ({ queued: false }));
export const useQueuedAfterSpeech = () => useQueued((s) => s.queued);

/**
 * Runs `run` once the speech model is downloaded (or its download failed), so
 * a second big download doesn't slow the first dictation.
 */
export const runAfterSpeechModel = (run: () => void) => {
  if (isFlowPreview) {
    useQueued.setState({ queued: true });
    return;
  }
  if (speechSettled()) return run();
  if (useQueued.getState().queued) return;
  useQueued.setState({ queued: true });
  const go = () => {
    if (!speechSettled()) return;
    stopModels();
    stopFailed();
    useQueued.setState({ queued: false });
    run();
  };
  const stopModels = useModelStore.subscribe(go);
  const stopFailed = useFailed.subscribe(go);
};

export interface SpeechChoice {
  /** The model setup downloads. */
  id: string;
  name: string;
  /** The Mac's language, e.g. "fr". */
  language: string;
  /** The model for the other kind of Mac: English or many languages. */
  other: { id: string; name: string } | null;
}

/** Which speech model setup uses and why, plus the one to switch to. */
export const useSpeechChoice = (): SpeechChoice | null => {
  const models = useModelStore((s) => s.models);
  const { language } = useChoice();
  const list = isFlowPreview ? PREVIEW_MODELS : models;
  const model = pickDefault(list);
  if (!model || language === null) return null;
  const forMac = modelForLanguage(language, list);
  const english = model.id === DEFAULT_MODEL_ID;
  const otherId = !english
    ? DEFAULT_MODEL_ID
    : forMac === DEFAULT_MODEL_ID
      ? WORLD_MODEL_ID
      : forMac;
  const other = list.find((m) => m.id === otherId && m.id !== model.id);
  return {
    id: model.id,
    name: model.name,
    language: english ? "en" : baseLanguage(language),
    other: other ? { id: other.id, name: other.name } : null,
  };
};

const previewModel = (id: string, name: string, languages: string[]) =>
  ({ id, name, supported_languages: languages }) as unknown as ModelInfo;

const PREVIEW_MODELS: ModelInfo[] = [
  previewModel(DEFAULT_MODEL_ID, "Parakeet Unified English 0.6B", ["en"]),
  previewModel(EUROPEAN_MODEL_ID, "Parakeet TDT 0.6B v3", ["fr", "de", "es"]),
  previewModel(WORLD_MODEL_ID, "Whisper Large v3 Turbo", ["hi", "ja"]),
];

export const useModelSetup = (): ModelSetup => {
  const models = useModelStore((s) => s.models);
  const currentModel = useModelStore((s) => s.currentModel);
  const downloading = useModelStore((s) => s.downloadingModels);
  const verifying = useModelStore((s) => s.verifyingModels);
  const progress = useModelStore((s) => s.downloadProgress);
  const { failed, reason } = useFailed();
  useChoice();
  const model = pickDefault(models);

  if (isFlowPreview) {
    return { status: "downloading", percent: 42, name: "Parakeet Unified" };
  }
  if (!model) return { status: "waiting", percent: 0, name: "" };
  const name = model.name;
  if (currentModel && currentModel !== model.id) {
    return { status: "ready", percent: 100, name };
  }
  if (model.id in verifying) return { status: "verifying", percent: 100, name };
  if (model.id in downloading) {
    return {
      status: "downloading",
      percent: Math.round(progress[model.id]?.percentage ?? 0),
      name,
    };
  }
  if (model.is_downloaded) {
    return {
      status: currentModel === model.id ? "ready" : "loading",
      percent: 100,
      name,
    };
  }
  return failed
    ? { status: "failed", percent: 0, name, reason }
    : { status: "waiting", percent: 0, name };
};
