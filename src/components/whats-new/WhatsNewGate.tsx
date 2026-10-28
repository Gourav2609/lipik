import React, { useCallback, useEffect, useRef, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { commands } from "@/bindings";
import { whatsNewPreview } from "@/flow/dev/preview";
import { WhatsNewSheet } from "@/flow/WhatsNewSheet";
import { useSettings } from "../../hooks/useSettings";
import { findLatestReleaseNote, findReleaseNoteToShow } from "./releaseNotes";
import type { ReleaseNote } from "./releaseNotes";

export const WhatsNewGate: React.FC = () => {
  const { settings, isLoading, updateSetting } = useSettings();
  const [note, setNote] = useState<ReleaseNote | null>(null);
  const [version, setVersion] = useState("");
  const dismissedVersionRef = useRef<string | null>(null);
  const raisedVersionRef = useRef<string | null>(null);

  useEffect(() => {
    if (whatsNewPreview) {
      const latest = findLatestReleaseNote();
      setNote(latest);
      setVersion(latest?.version ?? "");
      return;
    }
    if (isLoading || !settings || !settings.show_whats_new_on_update) {
      setNote(null);
      return;
    }

    let cancelled = false;

    const loadReleaseNote = async () => {
      try {
        const currentVersion = await getVersion();
        if (cancelled) return;

        const releaseNote = findReleaseNoteToShow({
          currentVersion,
          lastSeenVersion: settings.whats_new_last_seen_version ?? "",
        });

        if (
          !releaseNote ||
          dismissedVersionRef.current === releaseNote.version
        ) {
          setNote(null);
          return;
        }

        setVersion(currentVersion);
        setNote(releaseNote);
        // After an update restart the app may have started hidden in the
        // menu bar; bring the window forward so the notes are seen.
        if (raisedVersionRef.current !== releaseNote.version) {
          raisedVersionRef.current = releaseNote.version;
          void commands.showMainWindowCommand().catch(() => {});
        }
      } catch (error) {
        console.error("Failed to load release notes:", error);
      }
    };

    void loadReleaseNote();

    return () => {
      cancelled = true;
    };
  }, [
    isLoading,
    settings,
    settings?.show_whats_new_on_update,
    settings?.whats_new_last_seen_version,
  ]);

  const dismiss = useCallback(() => {
    if (!note) return;

    dismissedVersionRef.current = note.version;
    setNote(null);
    if (!whatsNewPreview)
      void updateSetting("whats_new_last_seen_version", note.version);
  }, [note, updateSetting]);

  if (!note) return null;

  return <WhatsNewSheet note={note} version={version} onDismiss={dismiss} />;
};
