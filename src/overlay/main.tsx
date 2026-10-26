import "@/lib/storageKeys";
import React from "react";
import ReactDOM from "react-dom/client";
import { listen } from "@tauri-apps/api/event";
import RecordingOverlay from "./RecordingOverlay";
import Pill from "./Pill";
import {
  applyTheme,
  getStoredTheme,
  syncThemeFromSettings,
} from "@/lib/utils/theme";
import type { Theme } from "@/bindings";
import "@/i18n";

// A separate webview from the settings window, so the overlay has to set
// `data-theme` on its own document: last-known theme before render (shared
// localStorage) to avoid a flash, reconcile with the persisted setting in case
// the overlay booted first, then follow live changes.
const boot = () => {
  applyTheme(getStoredTheme());
  syncThemeFromSettings();
  listen<Theme>("theme-changed", (event) => applyTheme(event.payload));

  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      {navigator.userAgent.includes("Mac") ? <Pill /> : <RecordingOverlay />}
    </React.StrictMode>,
  );
};

if (import.meta.env.DEV) import("./pill/preview").then(boot);
else boot();
