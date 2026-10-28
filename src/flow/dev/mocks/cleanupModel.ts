import type {
  CleanupEngine,
  CleanupModelState,
  CleanupModelStatus,
} from "@/bindings";
import type { MockHandler } from "../mockTauri";

// The clean-up model check in the preview. `?cleanupModel=no_model`,
// `downloading`, `verifying` or `failed` (the download button then fails), and with
// `?cleanupEngine=ollama` also `no_ollama` or `slow_model`; the default is
// "ready". The engine defaults to "builtin"; `?cleanupSmall=1` shows the 4B
// model that Macs with less than 16 GB get.
const params = new URLSearchParams(window.location.search);
const param = params.get("cleanupModel");
let engine: CleanupEngine =
  params.get("cleanupEngine") === "ollama" ? "ollama" : "builtin";

const state = (
  param === "failed" ? "no_model" : (param ?? "ready")
) as CleanupModelState;

const BUILTIN =
  params.get("cleanupSmall") === "1"
    ? { model: "Qwen3 4B Instruct", size: 2_497_281_120 }
    : { model: "Qwen3 8B", size: 5_027_783_488 };

const status = (): CleanupModelStatus => {
  const builtin = engine === "builtin";
  const total = builtin ? BUILTIN.size : 5_225_374_496;
  return {
    engine,
    builtin_available: true,
    failed: null,
    state,
    model: builtin
      ? BUILTIN.model
      : state === "slow_model"
        ? "qwen3:4b"
        : "qwen3:8b",
    progress:
      state === "downloading"
        ? { completed: Math.round(total * 0.42), total }
        : state === "verifying"
          ? { completed: total, total }
          : null,
    size: builtin ? BUILTIN.size : 0,
  };
};

export const handlers: Record<string, MockHandler> = {
  cleanup_model_status: status,
  pull_cleanup_model: () => {
    if (param === "failed")
      throw engine === "builtin"
        ? "Check your internet, then try again. (connection reset)"
        : "Ollama couldn't reach the internet.";
    return true;
  },
  cancel_cleanup_model_pull: () => null,
  set_cleanup_engine: (args) => {
    engine = args.engine as CleanupEngine;
    return null;
  },
};
