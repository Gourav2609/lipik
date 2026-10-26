import { useEffect, useState, type ComponentType } from "react";
import { getVersion } from "@tauri-apps/api/app";
import {
  CalendarClock,
  Info,
  Link2,
  Lock,
  Mic,
  PanelBottom,
  SquareCode,
  UserRound,
  Workflow,
  X,
  type LucideProps,
} from "lucide-react";
import { SETTINGS_PAGES } from "./settings";
import { MENU_EXIT_MS } from "./TopBar";
import { useFlowStore, type SettingsTab } from "./state";
import { IconButton } from "./ui";
import { APP_NAME } from "@/flow/brand";
import "./overlays/overlays.css";
import "./settings/settings.css";

type NavEntry = {
  id: SettingsTab;
  label: string;
  Icon: ComponentType<LucideProps>;
};

const GROUPS: NavEntry[][] = [
  [
    { id: "general", label: "Dictation", Icon: Mic },
    { id: "notetaker", label: "Meetings", Icon: CalendarClock },
    { id: "vibe-coding", label: "Code editors", Icon: SquareCode },
    { id: "connectors", label: "Connections", Icon: Link2 },
    { id: "mcp", label: "AI tools", Icon: Workflow },
  ],
  [
    { id: "system", label: "App", Icon: PanelBottom },
    { id: "privacy", label: "Privacy", Icon: Lock },
    { id: "account", label: "Profile", Icon: UserRound },
  ],
];

const ABOUT: NavEntry = { id: "billing", label: "About", Icon: Info };

const useAppVersion = () => {
  const [version, setVersion] = useState("");
  useEffect(() => {
    getVersion()
      .then(setVersion)
      .catch(() => setVersion(""));
  }, []);
  return version;
};

const NavButton = ({
  entry: { id, label, Icon },
  active,
  onClick,
}: {
  entry: NavEntry;
  active: boolean;
  onClick: () => void;
}) => (
  <button
    type="button"
    onClick={onClick}
    aria-current={active ? "page" : undefined}
    className={`fk-nav-item relative flex h-[34px] w-full items-center gap-[10px] rounded-[var(--fk-r-sm)] pl-[12px] text-[14.5px] font-medium leading-none ${active ? "bg-[var(--fk-menu-bg)] text-[var(--fk-text)] shadow-[var(--fk-elev-1)]" : "text-[var(--fk-text-muted)] hover:bg-[var(--fk-hover)] hover:text-[var(--fk-text)]"}`}
  >
    <Icon
      size={18}
      strokeWidth={1.75}
      className={`shrink-0 ${active ? "text-[var(--fk-accent)]" : ""}`}
    />
    {label}
  </button>
);

export const SettingsModal = () => {
  const tab = useFlowStore((s) => s.settingsTab);
  const openSettings = useFlowStore((s) => s.openSettings);
  const closeSettings = useFlowStore((s) => s.closeSettings);
  const version = useAppVersion();
  const [closing, setClosing] = useState(false);

  useEffect(() => setClosing(false), [tab]);

  useEffect(() => {
    if (!closing) return;
    const id = setTimeout(closeSettings, MENU_EXIT_MS);
    return () => clearTimeout(id);
  }, [closing, closeSettings]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || e.defaultPrevented) return;
      if (useFlowStore.getState().overlay) return;
      setClosing(true);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  if (!tab) return null;
  const Page = SETTINGS_PAGES[tab];

  return (
    <div
      className={`absolute inset-0 z-30 flex items-center justify-center ${closing ? "pointer-events-none" : ""}`}
      data-fk-closing={closing || undefined}
      onMouseDown={() => setClosing(true)}
    >
      <div className="fk-scrim-in absolute inset-0 bg-[var(--fk-scrim)]" />
      <div
        role="dialog"
        aria-label="Settings"
        className="fk-dialog relative flex h-[min(640px,calc(100%-64px))] w-[min(920px,calc(100%-64px))] overflow-hidden rounded-[var(--fk-r-xl)] bg-[var(--fk-panel)] shadow-[var(--fk-elev-3)]"
        onMouseDown={(e) => e.stopPropagation()}
      >
        <nav className="flex w-[216px] shrink-0 flex-col border-r border-[var(--fk-border)] bg-[var(--fk-bg)] px-[12px] pb-[16px] pt-[24px]">
          <div className="fk-display px-[12px] text-[20px] leading-[28px]">
            Settings
          </div>
          {GROUPS.map((group, i) => (
            <div
              key={group[0].id}
              className={i === 0 ? "mt-[20px]" : "mt-[12px] border-t border-[var(--fk-border)] pt-[12px]"}
            >
              <div className="flex flex-col gap-[2px]">
                {group.map((entry) => (
                  <NavButton
                    key={entry.id}
                    entry={entry}
                    active={tab === entry.id}
                    onClick={() => openSettings(entry.id)}
                  />
                ))}
              </div>
            </div>
          ))}
          <div className="mt-auto border-t border-[var(--fk-border)] pt-[12px]">
            <NavButton
              entry={ABOUT}
              active={tab === ABOUT.id}
              onClick={() => openSettings(ABOUT.id)}
            />
            <div className="fk-num mt-[8px] px-[12px] text-[12px] leading-[16px] text-[var(--fk-text-faint)]">
              {APP_NAME}
              {version && ` ${version}`}
            </div>
          </div>
        </nav>
        <div className="relative min-w-0 flex-1">
          <Page />
          <IconButton
            label="Close"
            onClick={() => setClosing(true)}
            className="absolute right-[16px] top-[16px]"
          >
            <X size={18} strokeWidth={1.75} />
          </IconButton>
        </div>
      </div>
    </div>
  );
};
