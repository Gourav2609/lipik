import { useCallback, useEffect, useRef, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { platform } from "@tauri-apps/plugin-os";
import {
  checkAccessibilityPermission,
  checkMicrophonePermission,
  requestAccessibilityPermission,
  requestMicrophonePermission,
} from "tauri-plugin-macos-permissions-api";
import { commands } from "@/bindings";
import { isFlowPreview } from "../dev/preview";

export type PermissionKind = "microphone" | "accessibility";

export interface PermissionState {
  microphone: boolean | null;
  accessibility: boolean | null;
}

const isMac = () => {
  try {
    return platform() === "macos";
  } catch {
    return false;
  }
};

const readMicrophone = async (): Promise<boolean> => {
  if (isMac()) return checkMicrophonePermission();
  try {
    const status = await commands.getWindowsMicrophonePermissionStatus();
    return !status.supported || status.overall_access !== "denied";
  } catch {
    return true;
  }
};

const readAccessibility = async (): Promise<boolean> =>
  isMac() ? checkAccessibilityPermission() : true;

let shortcutsReady = false;
const initShortcuts = () => {
  if (shortcutsReady || isFlowPreview) return;
  shortcutsReady = true;
  Promise.all([commands.initializeEnigo(), commands.initializeShortcuts()]).catch(
    (e) => {
      shortcutsReady = false;
      console.warn("Failed to initialize shortcuts:", e);
    },
  );
};

const PANES: Record<PermissionKind, string> = {
  microphone: "Privacy_Microphone",
  accessibility: "Privacy_Accessibility",
};

/** Opens System Settings at the Privacy & Security pane for this permission. */
export const openPrivacyPane = (kind: PermissionKind) =>
  openUrl(
    `x-apple.systempreferences:com.apple.preference.security?${PANES[kind]}`,
  ).catch((e) => console.warn("Failed to open System Settings:", e));

const asked = new Set<PermissionKind>();
const TIMEOUT_MS = 30_000;

const PREVIEW: PermissionState = { microphone: true, accessibility: false };

/** Live permission status, re-checked every second while mounted. */
export const usePermissions = () => {
  const [state, setState] = useState<PermissionState>(
    isFlowPreview ? PREVIEW : { microphone: null, accessibility: null },
  );

  const refresh = useCallback(async () => {
    if (isFlowPreview) return;
    try {
      const [microphone, accessibility] = await Promise.all([
        readMicrophone(),
        readAccessibility(),
      ]);
      if (accessibility) initShortcuts();
      setState((prev) =>
        prev.microphone === microphone && prev.accessibility === accessibility
          ? prev
          : { microphone, accessibility },
      );
    } catch (e) {
      console.warn("Failed to check permissions:", e);
    }
  }, []);

  useEffect(() => {
    refresh();
    const id = setInterval(refresh, 1000);
    return () => clearInterval(id);
  }, [refresh]);

  const [timedOut, setTimedOut] = useState<PermissionKind | null>(null);
  const waiting = useRef<{ kind: PermissionKind; since: number } | null>(null);

  useEffect(() => {
    const w = waiting.current;
    if (w && state[w.kind]) {
      waiting.current = null;
      setTimedOut(null);
    }
  }, [state]);

  const request = useCallback(
    async (kind: PermissionKind) => {
      if (isFlowPreview) return;
      setTimedOut(null);
      const w = { kind, since: Date.now() };
      waiting.current = w;
      setTimeout(() => {
        if (waiting.current === w) setTimedOut(kind);
      }, TIMEOUT_MS);
      try {
        if (kind === "microphone") {
          if (!isMac()) await commands.openMicrophonePrivacySettings();
          else if (asked.has("microphone")) await openPrivacyPane("microphone");
          else await requestMicrophonePermission();
        } else if (isMac()) {
          await requestAccessibilityPermission();
        }
      } catch (e) {
        console.warn(`Failed to request ${kind} permission:`, e);
      }
      asked.add(kind);
      refresh();
    },
    [refresh],
  );

  const needsAccessibility = isMac();
  return { ...state, request, timedOut, needsAccessibility };
};
