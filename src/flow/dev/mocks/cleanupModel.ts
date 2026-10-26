import type { CleanupModelState, CleanupModelStatus } from "@/bindings";
import type { MockHandler } from "../mockTauri";

// The clean-up model check in the preview. `?cleanupModel=no_ollama`,
// `no_model`, `slow_model`, `downloading` or `failed` (the download button then fails);
// the default is "ready".
const param = new URLSearchParams(window.location.search).get("cleanupModel");

const state = (
  param === "failed" ? "no_model" : (param ?? "ready")
) as CleanupModelState;

const status = (): CleanupModelStatus => ({
  state,
  model: state === "slow_model" ? "qwen3:4b" : "qwen3:8b",
  progress:
    state === "downloading"
      ? { completed: 2_210_000_000, total: 5_225_374_496 }
      : null,
});

export const handlers: Record<string, MockHandler> = {
  cleanup_model_status: status,
  pull_cleanup_model: () => {
    if (param === "failed") throw "Ollama couldn't reach the internet.";
    return true;
  },
  cancel_cleanup_model_pull: () => null,
};
