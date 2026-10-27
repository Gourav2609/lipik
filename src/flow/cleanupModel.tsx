import { useCallback, useEffect, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  commands,
  events,
  type CleanupEngine,
  type CleanupModelPull,
  type CleanupModelStatus,
} from "@/bindings";
import { Button } from "./ui";

export const OLLAMA_DOWNLOAD_URL = "https://ollama.com/download";

const RECHECK_MS = 3000;
const READY_RECHECK_MS = 30000;

export type CleanupModelView =
  | "checking"
  | "no_ollama"
  | "no_model"
  | "slow_model"
  | "downloading"
  | "verifying"
  | "failed"
  | "ready";

export const SLOW_MODEL_TEXT =
  "qwen3:4b thinks before answering and is too slow for dictation; use qwen3:4b-instruct or qwen3:8b.";

export const openOllamaDownload = () => {
  openUrl(OLLAMA_DOWNLOAD_URL).catch(() => {});
};

const GB = 1e9;

/** The download button: the built-in model's size, or the Ollama model. */
export const downloadLabel = (model: string, size = 0) =>
  size > 0
    ? `Download the clean-up model (${(size / GB).toFixed(1)} GB)`
    : model === "qwen3:8b"
      ? "Download the clean-up model (5 GB)"
      : `Download ${model}`;

export const pullText = (p: CleanupModelPull | null) =>
  p && p.total > 0
    ? `${(p.completed / GB).toFixed(1)} of ${(p.total / GB).toFixed(1)} GB`
    : "Starting…";

export const pullPercent = (p: CleanupModelPull | null) =>
  p && p.total > 0 ? Math.floor((p.completed / p.total) * 100) : 0;

/**
 * Which engine runs clean-up and whether it has its model (for Ollama:
 * whether Ollama runs and has it), checked again every few seconds until it
 * does (and now and then after), plus the model's download.
 */
export const useCleanupModel = () => {
  const [status, setStatus] = useState<CleanupModelStatus | null>(null);
  const [progress, setProgress] = useState<CleanupModelPull | null>(null);
  const [error, setError] = useState<{ model: string; text: string } | null>(
    null,
  );
  const alive = useRef(true);
  const sent = useRef(0);
  const oldest = useRef(0);

  const check = useCallback(async () => {
    const n = ++sent.current;
    const s = await commands.cleanupModelStatus().catch(() => null);
    if (!alive.current || !s || n < oldest.current) return;
    setStatus(s);
    if (s.progress) setProgress(s.progress);
  }, []);

  useEffect(() => {
    alive.current = true;
    check();
    const unlisten = events.cleanupModelPull.listen(({ payload }) =>
      setProgress(payload),
    );
    return () => {
      alive.current = false;
      unlisten.then((fn) => fn());
    };
  }, [check]);

  const state = status?.state;
  useEffect(() => {
    if (!state) return;
    const id = setInterval(
      check,
      state === "ready" ? READY_RECHECK_MS : RECHECK_MS,
    );
    return () => clearInterval(id);
  }, [state, check]);

  const download = useCallback(async () => {
    oldest.current = sent.current + 1;
    setError(null);
    setProgress(null);
    setStatus((s) => s && { ...s, state: "downloading", progress: null });
    const model = status?.model ?? "";
    const res = await commands.pullCleanupModel();
    if (!alive.current) return;
    if (res.status === "error") setError({ model, text: res.error });
    await check();
  }, [check, status?.model]);

  const cancel = useCallback(async () => {
    await commands.cancelCleanupModelPull();
    await check();
  }, [check]);

  const setEngine = useCallback(
    async (engine: CleanupEngine) => {
      oldest.current = sent.current + 1;
      setError(null);
      setProgress(null);
      await commands.setCleanupEngine(engine);
      await check();
    },
    [check],
  );

  const failed =
    error !== null &&
    status?.state === "no_model" &&
    error.model === status.model;
  const view: CleanupModelView = !status
    ? "checking"
    : failed
      ? "failed"
      : status.state;

  return {
    view,
    engine: status?.engine ?? null,
    builtinAvailable: status?.builtin_available ?? false,
    model: status?.model ?? "",
    size: status?.size ?? 0,
    progress,
    error: failed ? error.text : null,
    download,
    cancel,
    setEngine,
    recheck: check,
  };
};

/** The download's progress bar, percent and size, with a Cancel button. */
export const PullProgress = ({
  progress,
  onCancel,
}: {
  progress: CleanupModelPull | null;
  onCancel: () => void;
}) => {
  const percent = pullPercent(progress);
  return (
    <div className="flex items-center gap-[12px]">
      <div className="min-w-0 flex-1">
        <div
          className="h-[6px] overflow-hidden rounded-full bg-[var(--fk-surface-2)]"
          role="progressbar"
          aria-label="Clean-up model download"
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={percent}
        >
          <div
            className="h-full rounded-full bg-[var(--fk-accent)] transition-[width] duration-[var(--fk-t-base)]"
            style={{ width: `${percent}%` }}
          />
        </div>
        <div className="fk-num mt-[6px] flex justify-between text-[13px] leading-[18px] text-[var(--fk-text-muted)]">
          <span>{pullText(progress)}</span>
          <span>{percent}%</span>
        </div>
      </div>
      <Button variant="secondary" size="sm" onClick={onCancel}>
        Cancel
      </Button>
    </div>
  );
};
