import { useEffect, useRef, useState, type ReactNode } from "react";
import {
  CalendarDays,
  ChevronLeft,
  Copy,
  Ellipsis,
  ExternalLink,
  FileText,
  History,
  Share2,
  Trash2,
} from "lucide-react";
import { commands } from "@/bindings";
import { APP_NAME } from "@/flow/brand";

type Props = {
  showCalendar: boolean;
  chatOpen: boolean;
  onConnectCalendar: () => void;
  onShare: () => void;
  onCopyMarkdown: () => void;
  onCopySummary: () => void;
  onToggleChats: () => void;
  onDelete: () => Promise<string | null>;
};

const openHub = () => commands.openMainPage("notetaker").catch(() => {});

export const TopBar = (p: Props) => {
  const [menu, setMenu] = useState(false);
  const [confirm, setConfirm] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!menu) return;
    const close = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) setMenu(false);
    };
    const esc = (e: KeyboardEvent) => e.key === "Escape" && setMenu(false);
    window.addEventListener("mousedown", close);
    window.addEventListener("keydown", esc);
    return () => {
      window.removeEventListener("mousedown", close);
      window.removeEventListener("keydown", esc);
    };
  }, [menu]);

  useEffect(() => {
    if (!menu) {
      setConfirm(false);
      setError(null);
    }
  }, [menu]);

  const item = (
    icon: ReactNode,
    label: string,
    run: () => void,
  ) => (
    <button
      type="button"
      className="np-menu-item"
      role="menuitem"
      onClick={() => {
        run();
        setMenu(false);
      }}
    >
      {icon} {label}
    </button>
  );

  return (
    <div className="np-bar" data-tauri-drag-region>
      <button
        type="button"
        className="np-icon-btn"
        aria-label="Back to Meetings"
        title="Back to Meetings"
        onClick={openHub}
      >
        <ChevronLeft size={18} strokeWidth={1.75} />
      </button>
      <div className="np-bar-gap" data-tauri-drag-region />
      <button
        type="button"
        className="np-icon-btn"
        aria-label="Earlier questions"
        aria-pressed={p.chatOpen}
        title="Earlier questions"
        onClick={p.onToggleChats}
      >
        <History size={18} strokeWidth={1.75} />
      </button>
      <div ref={ref} style={{ position: "relative" }}>
        <button
          type="button"
          className="np-icon-btn"
          aria-label="More"
          title="More"
          aria-expanded={menu}
          onClick={() => setMenu((m) => !m)}
        >
          <Ellipsis size={18} strokeWidth={1.75} />
        </button>
        {menu && (
          <div className="np-menu" style={{ top: 34, right: 0, transformOrigin: "top right" }} role="menu">
            {!confirm ? (
              <>
                {item(
                  <Copy size={16} strokeWidth={1.75} />,
                  "Copy summary",
                  p.onCopySummary,
                )}
                {item(
                  <Share2 size={16} strokeWidth={1.75} />,
                  "Copy summary and transcript",
                  p.onShare,
                )}
                {item(
                  <FileText size={16} strokeWidth={1.75} />,
                  "Copy as Markdown",
                  p.onCopyMarkdown,
                )}
                <div className="np-menu-rule" />
                {p.showCalendar &&
                  item(
                    <CalendarDays size={16} strokeWidth={1.75} />,
                    "Connect calendar",
                    p.onConnectCalendar,
                  )}
                {item(
                  <ExternalLink size={16} strokeWidth={1.75} />,
                  `Open in ${APP_NAME}`,
                  openHub,
                )}
                <div className="np-menu-rule" />
                <button
                  type="button"
                  className="np-menu-item danger"
                  role="menuitem"
                  onClick={() => setConfirm(true)}
                >
                  <Trash2 size={16} strokeWidth={1.75} /> Delete note
                </button>
              </>
            ) : (
              <>
                <div className="np-menu-note">
                  {error ??
                    "Delete this note? Its audio, transcript and notes go with it. This can't be undone."}
                </div>
                <div className="np-menu-actions">
                  <button
                    type="button"
                    className="np-btn ghost"
                    onClick={() => setMenu(false)}
                  >
                    Keep
                  </button>
                  <button
                    type="button"
                    className="np-btn danger"
                    onClick={async () => {
                      const e = await p.onDelete();
                      if (e) setError(e);
                      else setMenu(false);
                    }}
                  >
                    Delete
                  </button>
                </div>
              </>
            )}
          </div>
        )}
      </div>
    </div>
  );
};
