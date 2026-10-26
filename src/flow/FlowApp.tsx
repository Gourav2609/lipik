import "./flow.css";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useEffect, useState } from "react";
import { isFlowPreview, LOOK } from "./dev/preview";
import { OVERLAYS } from "./overlays";
import { PAGES } from "./pages";
import { SettingsModal } from "./SettingsModal";
import { SecureInputNotice } from "./SecureInputNotice";
import { Sidebar } from "./Sidebar";
import { type OverlayId, type PageId, useFlowStore } from "./state";
import { FakeTrafficLights, MENU_EXIT_MS, TopBar, useLingering } from "./TopBar";
import { useResolvedTheme } from "./useResolvedTheme";

const MENUS: OverlayId[] = ["account-menu", "theme-menu"];

const previewFullscreen =
  isFlowPreview &&
  new URLSearchParams(window.location.search).get("fullscreen") === "1";

/** In native fullscreen there are no window buttons, so the brand moves to the left edge. */
const useFullscreen = () => {
  const [fullscreen, setFullscreen] = useState(previewFullscreen);
  useEffect(() => {
    if (isFlowPreview) return;
    const win = getCurrentWindow();
    let pending = false;
    let again = false;
    const check = () => {
      if (pending) {
        again = true;
        return;
      }
      pending = true;
      win
        .isFullscreen()
        .then(setFullscreen)
        .catch(() => {})
        .finally(() => {
          pending = false;
          if (again) {
            again = false;
            check();
          }
        });
    };
    check();
    const unlisten = win.onResized(check);
    return () => {
      unlisten.then((f) => f());
    };
  }, []);
  return fullscreen;
};

export const FlowApp = () => {
  const theme = useResolvedTheme();
  const page = useFlowStore((s) => s.page);
  const collapsed = useFlowStore((s) => s.sidebarCollapsed);
  const fullscreen = useFullscreen();
  const settingsTab = useFlowStore((s) => s.settingsTab);
  const overlay = useFlowStore((s) => s.overlay);
  const closeOverlay = useFlowStore((s) => s.closeOverlay);
  const setPage = useFlowStore((s) => s.setPage);
  const openSettings = useFlowStore((s) => s.openSettings);
  const openOverlay = useFlowStore((s) => s.openOverlay);
  const Page = PAGES[page];
  const [shown, closing] = useLingering(overlay, MENU_EXIT_MS);
  const Overlay =
    shown && (!closing || (MENUS.includes(shown) && !settingsTab))
      ? OVERLAYS[shown]
      : null;

  useEffect(() => {
    const unlisten = listen<string>("flow-open-page", (e) => {
      if (e.payload === "settings") openSettings();
      else if (e.payload === "connect-calendar") openOverlay("connect-calendar");
      else if (e.payload in PAGES) setPage(e.payload as PageId);
    });
    return () => {
      unlisten.then((f) => f());
    };
  }, [setPage, openSettings, openOverlay]);

  return (
    <div
      className="fk relative h-screen w-screen select-none overflow-hidden"
      data-fk-theme={theme}
      data-look={LOOK}
      data-fk-collapsed={collapsed || undefined}
    >
      <Sidebar fullscreen={fullscreen} />
      <main className="fk-main absolute inset-y-0 right-0 overflow-hidden border-l border-[var(--fk-border)] bg-[var(--fk-panel)]">
        <div className="fk-scroll absolute inset-x-0 bottom-0 top-[var(--fk-header-h)]">
          <SecureInputNotice />
          <Page />
        </div>
      </main>
      <TopBar />
      {settingsTab && <SettingsModal />}
      {Overlay && (
        <div data-fk-closing={closing || undefined}>
          <Overlay onClose={closeOverlay} />
        </div>
      )}
      {isFlowPreview && !fullscreen && <FakeTrafficLights />}
    </div>
  );
};
