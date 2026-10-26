import { listen } from "@tauri-apps/api/event";
import type { Result } from "@/bindings";

export type TextflowKind = "dictionary" | "snippets" | "style" | "transforms";

/**
 * Calls `fn` when the backend reports a change to that list, once per burst
 * (an import sends one event per item). Returns an unsubscribe.
 */
export const onTextflowChanged = (kind: TextflowKind, fn: () => void) => {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const unlisten = listen<string>("textflow-changed", (e) => {
    if (e.payload !== kind) return;
    clearTimeout(timer);
    timer = setTimeout(fn, 150);
  });
  return () => {
    clearTimeout(timer);
    void unlisten.then((stop) => stop());
  };
};

export const unwrap = <T>(res: Result<T, string>): T => {
  if (res.status === "error") throw new Error(res.error);
  return res.data;
};

export const errorText = (err: unknown) =>
  err instanceof Error ? err.message : String(err);

// Backend ids are 11 hex digits of epoch millis, then a hex counter that restarts with the app.
const idTime = (id: string) => [
  parseInt(id.slice(0, 11), 16) || 0,
  parseInt(id.slice(11), 16) || 0,
];

export const compareIdsNewestFirst = (a: string, b: string) => {
  const [ta, na] = idTime(a);
  const [tb, nb] = idTime(b);
  return tb - ta || nb - na;
};
