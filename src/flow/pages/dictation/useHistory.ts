import { useCallback, useEffect, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { readFile } from "@tauri-apps/plugin-fs";
import { type } from "@tauri-apps/plugin-os";
import { commands, events, type HistoryEntry } from "@/bindings";

const PAGE_SIZE = 100;
const MAX_PAGES = 50;

export const entryText = (e: HistoryEntry) =>
  (e.corrected_text ?? e.post_processed_text ?? e.transcription_text).trim();

export const getAudioUrl = async (fileName: string) => {
  try {
    const result = await commands.getAudioFilePath(fileName);
    if (result.status !== "ok" || !result.data) return null;
    if (type() === "linux") {
      const data = await readFile(result.data);
      return URL.createObjectURL(new Blob([data], { type: "audio/wav" }));
    }
    return convertFileSrc(result.data, "asset");
  } catch (error) {
    console.error("Failed to get audio file path:", error);
    return null;
  }
};

/** Frees a URL from getAudioUrl (Linux returns a blob URL per call). */
export const releaseAudioUrl = (url: string) => {
  if (url.startsWith("blob:")) URL.revokeObjectURL(url);
};

/** All saved history, newest first, kept in sync with the transcription pipeline. */
export const useHistory = () => {
  const [entries, setEntries] = useState<HistoryEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const loadId = useRef(0);

  const load = useCallback(async () => {
    const id = ++loadId.current;
    const all: HistoryEntry[] = [];
    let cursor: number | null = null;
    try {
      for (let page = 0; page < MAX_PAGES; page++) {
        const result = await commands.getHistoryEntries(cursor, PAGE_SIZE);
        if (result.status !== "ok") break;
        all.push(...result.data.entries);
        if (!result.data.has_more || result.data.entries.length === 0) break;
        cursor = result.data.entries[result.data.entries.length - 1].id;
      }
    } catch (error) {
      console.error("Failed to load history:", error);
    }
    if (id !== loadId.current) return;
    setEntries(all);
    setLoading(false);
  }, []);

  useEffect(() => {
    load();
    const unlisten = events.historyUpdatePayload.listen(({ payload }) => {
      if (payload.action === "updated") {
        setEntries((prev) =>
          prev.map((e) => (e.id === payload.entry.id ? payload.entry : e)),
        );
      } else {
        // Adding an entry can silently prune the oldest ones (history limit),
        // and deletes/toggles may come from another window, so re-read it all.
        load();
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [load]);

  const toggleSaved = useCallback(
    async (id: number) => {
      const flip = () =>
        setEntries((prev) =>
          prev.map((e) => (e.id === id ? { ...e, saved: !e.saved } : e)),
        );
      flip();
      const result = await commands.toggleHistoryEntrySaved(id).catch(() => null);
      if (result?.status !== "ok") flip();
    },
    [],
  );

  const remove = useCallback(
    async (id: number) => {
      setEntries((prev) => prev.filter((e) => e.id !== id));
      const result = await commands.deleteHistoryEntry(id).catch(() => null);
      if (result?.status !== "ok") load();
    },
    [load],
  );

  const retry = useCallback(async (id: number) => {
    const result = await commands.retryHistoryEntryTranscription(id);
    if (result.status !== "ok") throw new Error(String(result.error));
  }, []);

  /** Saves an edit of an entry's text. Resolves to what the edit taught the dictionary. */
  const updateText = useCallback(async (id: number, text: string) => {
    const result = await commands.updateHistoryText(id, text);
    if (result.status !== "ok") throw new Error(String(result.error));
    const { entry, learned_words, learned_replacements } = result.data;
    setEntries((prev) => prev.map((e) => (e.id === id ? entry : e)));
    return [...learned_words, ...learned_replacements];
  }, []);

  return { entries, loading, toggleSaved, remove, retry, updateText };
};
