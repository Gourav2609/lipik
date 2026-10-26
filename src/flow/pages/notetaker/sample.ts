// Preview-only (?flowPreview=1) helpers. The sample meetings and notes live in
// src/flow/dev/mocks/notetaker.ts.

const params = new URLSearchParams(window.location.search);

/** Preview-only URL switches for screenshots of each state. */
export const PREVIEW = {
  tab: params.get("ntTab"),
  select: params.get("ntSelect"),
  hideToday: params.get("ntToday") === "0",
  /** Opens a note's full view, e.g. ntOpen=n1&ntNoteTab=transcript. */
  open: params.get("ntOpen"),
  noteTab: params.get("ntNoteTab"),
  /** Asks this question on load. */
  ask: params.get("ntAsk"),
};
