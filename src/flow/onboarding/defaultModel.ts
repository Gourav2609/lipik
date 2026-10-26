import { create } from "zustand";
import type { ModelInfo } from "@/bindings";
import { useModelStore } from "@/stores/modelStore";
import { isFlowPreview } from "../dev/preview";

export const DEFAULT_MODEL_ID = "parakeet-unified-en-0.6b";

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
}

const pickDefault = (models: ModelInfo[]): ModelInfo | undefined =>
  models.find((m) => m.id === DEFAULT_MODEL_ID) ??
  models.find((m) => m.is_recommended) ??
  models[0];

const useFailed = create<{ failed: boolean }>(() => ({ failed: false }));
const setFailed = (failed: boolean) => useFailed.setState({ failed });

let started = false;
let selecting = false;

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
  if (useFailed.getState().failed) return;
  const ok = await store.downloadModel(id);
  if (!ok) setFailed(true);
};

let unsubscribe: (() => void) | undefined;

/** Downloads and selects the default model once, without any UI blocking on it. */
export const startDefaultModelSetup = () => {
  if (started || isFlowPreview) return;
  started = true;
  unsubscribe = useModelStore.subscribe(() => {
    step();
  });
  step();
};

/** Restarts a failed download (e.g. after the network came back). */
export const retryDefaultModelSetup = () => {
  setFailed(false);
  step();
};

export const useModelSetup = (): ModelSetup => {
  const models = useModelStore((s) => s.models);
  const currentModel = useModelStore((s) => s.currentModel);
  const downloading = useModelStore((s) => s.downloadingModels);
  const verifying = useModelStore((s) => s.verifyingModels);
  const progress = useModelStore((s) => s.downloadProgress);
  const failed = useFailed((s) => s.failed);
  const model = pickDefault(models);

  if (isFlowPreview) {
    return { status: "downloading", percent: 42, name: "Parakeet Unified" };
  }
  if (!model) return { status: "waiting", percent: 0, name: "" };
  const name = model.name;
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
  return { status: failed ? "failed" : "waiting", percent: 0, name };
};
