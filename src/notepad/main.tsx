import "@/lib/storageKeys";
import React from "react";
import ReactDOM from "react-dom/client";
import { listen } from "@tauri-apps/api/event";
import type { Theme } from "@/bindings";
import {
  applyTheme,
  getStoredTheme,
  syncThemeFromSettings,
} from "@/lib/utils/theme";
import { Notepad } from "./Notepad";
import "./notepad.css";

const boot = (frame: boolean) => {
  applyTheme(getStoredTheme());
  syncThemeFromSettings();
  listen<Theme>("theme-changed", (event) => applyTheme(event.payload));

  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <Notepad />
      {frame && (
        <div className="np-frame" aria-hidden>
          <i style={{ left: 16, background: "#FF5F57" }} />
          <i style={{ left: 39, background: "#FEBC2E" }} />
          <i style={{ left: 62, background: "#28C840" }} />
        </div>
      )}
    </React.StrictMode>,
  );
};

if (import.meta.env.DEV)
  import("./preview").then((m) => boot(m.isNotepadPreview));
else boot(false);
