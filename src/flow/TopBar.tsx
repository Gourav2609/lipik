import { Bell, CircleUser, PanelLeft, Sun } from "lucide-react";
import { useEffect, useState } from "react";
import { isFlowPreview } from "./dev/preview";
import Notifications from "./overlays/Notifications";
import { useFlowStore } from "./state";
import { IconButton } from "./ui";

export const MENU_EXIT_MS = 120;

/**
 * Keeps the last non-null value for `ms` after `value` turns null, so a
 * closing menu can play its exit animation. Returns [value, closing].
 */
export const useLingering = <T,>(
  value: T | null,
  ms: number,
): [T | null, boolean] => {
  const [shown, setShown] = useState(value);
  useEffect(() => {
    if (value !== null) {
      setShown(value);
      return;
    }
    const id = setTimeout(() => setShown(null), ms);
    return () => clearTimeout(id);
  }, [value, ms]);
  return value !== null ? [value, false] : [shown, shown !== null];
};

const initialNotifications =
  isFlowPreview &&
  new URLSearchParams(window.location.search).get("overlay") ===
    "notifications";

export const TopBar = () => {
  const overlay = useFlowStore((s) => s.overlay);
  const collapsed = useFlowStore((s) => s.sidebarCollapsed);
  const toggleSidebar = useFlowStore((s) => s.toggleSidebar);
  const openOverlay = useFlowStore((s) => s.openOverlay);
  const closeOverlay = useFlowStore((s) => s.closeOverlay);
  const settingsOpen = useFlowStore((s) => s.settingsTab !== null);
  const [notifications, setNotifications] = useState(initialNotifications);
  useEffect(() => {
    if (overlay || settingsOpen) setNotifications(false);
  }, [overlay, settingsOpen]);
  const [notificationsShown, notificationsClosing] = useLingering(
    notifications || null,
    MENU_EXIT_MS,
  );
  const toggle = (id: "theme-menu" | "account-menu") =>
    overlay === id ? closeOverlay() : openOverlay(id);

  return (
    <>
      <header
        data-tauri-drag-region
        className="fk-header absolute right-0 top-0 z-10 flex h-[var(--fk-header-h)] items-center gap-[4px] pl-[12px] pr-[12px]"
      >
        <IconButton
          label={collapsed ? "Show sidebar" : "Hide sidebar"}
          onClick={toggleSidebar}
        >
          <PanelLeft size={18} strokeWidth={1.75} />
        </IconButton>
        <div data-tauri-drag-region className="h-full flex-1" />
        <IconButton
          label="Appearance"
          active={overlay === "theme-menu"}
          onClick={() => toggle("theme-menu")}
        >
          <Sun size={18} strokeWidth={1.75} />
        </IconButton>
        <IconButton
          label="Notifications"
          active={notifications}
          onClick={() => {
            if (!notifications) closeOverlay();
            setNotifications(!notifications);
          }}
        >
          <Bell size={18} strokeWidth={1.75} />
        </IconButton>
        <IconButton
          label="Account"
          active={overlay === "account-menu"}
          onClick={() => toggle("account-menu")}
        >
          <CircleUser size={18} strokeWidth={1.75} />
        </IconButton>
      </header>
      {notificationsShown && (
        <div data-fk-closing={notificationsClosing || undefined}>
          <Notifications onClose={() => setNotifications(false)} />
        </div>
      )}
    </>
  );
};

/** Stand-in for the native macOS window buttons, drawn only in preview mode. */
export const FakeTrafficLights = ({ barHeight = 52 }: { barHeight?: number }) => (
  <div
    className="pointer-events-none absolute left-[16px] z-[60] flex gap-[8px]"
    style={{ top: (barHeight - 12) / 2 }}
  >
    {["#ff5f57", "#febc2e", "#28c840"].map((c) => (
      <span
        key={c}
        className="h-[12px] w-[12px] rounded-full"
        style={{
          background: c,
          boxShadow: "inset 0 0 0 0.5px rgba(0,0,0,0.25)",
        }}
      />
    ))}
  </div>
);
