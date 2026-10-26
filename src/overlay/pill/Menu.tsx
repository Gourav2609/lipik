import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import {
  AlarmClockCheck,
  Check,
  ChevronRight,
  ClipboardList,
  ClipboardPaste,
  Mic,
  Settings,
} from "lucide-react";
import { commands, type AudioDevice } from "@/bindings";

// Right-click menu, measured in S3 319.28-332.70 (ANIMATIONS.md §5.4): 252 x 190 pt
// with its border, 6 pt padding, 32 pt rows, 9 pt separator bands.
export const MENU = {
  w: 252,
  edge: 50,
  pad: 6,
  row: 32,
  sep: 9,
  subW: 180,
  subGap: 4,
};
export const MENU_H = 2 * MENU.pad + 5 * MENU.row + 2 * MENU.sep;
/** Top of the Microphone row inside the menu. */
export const MIC_ROW_TOP = MENU.pad + 2 * MENU.row + MENU.sep;
export const subHeight = (n: number) =>
  2 * MENU.pad + Math.max(1, n) * MENU.row;

export type MenuAction = "hide" | "settings" | "history" | "paste";

type Props = {
  style: React.CSSProperties;
  subStyle: (rows: number) => React.CSSProperties;
  hot: string | null;
  onHot: (row: string | null) => void;
  onAction: (action: MenuAction) => void;
  onMic: (name: string) => void;
};

const ICON = { size: 15, strokeWidth: 2 };

const same = (a: string, b: string) => a.toLowerCase() === b.toLowerCase();

export const Menu: React.FC<Props> = ({
  style,
  subStyle,
  hot,
  onHot,
  onAction,
  onMic,
}) => {
  const { t: tr } = useTranslation();
  const [mics, setMics] = useState<AudioDevice[]>([]);
  const [current, setCurrent] = useState("default");
  const [subOpen, setSubOpen] = useState(false);

  useEffect(() => {
    commands
      .getAvailableMicrophones()
      .then((r) => r.status === "ok" && setMics(r.data))
      .catch(() => {});
    commands
      .getSelectedMicrophone()
      .then((r) => r.status === "ok" && setCurrent(r.data))
      .catch(() => {});
  }, []);

  useEffect(() => {
    if (hot === "mic" || hot?.startsWith("mic:")) setSubOpen(true);
    else if (hot) setSubOpen(false);
  }, [hot]);

  const row = (
    id: string,
    icon: React.ReactNode,
    label: string,
    onClick: () => void,
    tail?: React.ReactNode,
  ) => (
    <button
      data-row={id}
      className={`menu-row ${hot === id || (id === "mic" && subOpen) ? "hot" : ""}`}
      onMouseMove={() => hot !== id && onHot(id)}
      onClick={onClick}
    >
      <span className="menu-icon">{icon}</span>
      <span className="menu-label">{label}</span>
      {tail}
    </button>
  );

  return (
    <>
      <div className="pill-menu" style={style}>
        {row(
          "hide",
          <AlarmClockCheck {...ICON} />,
          tr("pill.hideHour", "Hide for 1 hour"),
          () => onAction("hide"),
        )}
        {row(
          "settings",
          <Settings {...ICON} />,
          tr("pill.settings", "Settings"),
          () => onAction("settings"),
        )}
        <div className="menu-sep" />
        {row(
          "mic",
          <Mic {...ICON} />,
          tr("pill.microphone", "Microphone"),
          () => setSubOpen((o) => !o),
          <ChevronRight className="menu-more" size={17} strokeWidth={1.6} />,
        )}
        <div className="menu-sep" />
        {row(
          "history",
          <ClipboardList {...ICON} />,
          tr("pill.transcriptHistory", "Transcript history"),
          () => onAction("history"),
        )}
        {row(
          "paste",
          <ClipboardPaste {...ICON} />,
          tr("pill.pasteLast", "Paste last transcript"),
          () => onAction("paste"),
        )}
      </div>
      {subOpen && (
        <div className="pill-menu sub" style={subStyle(mics.length)}>
          {mics.map((m) => {
            const id = `mic:${m.name}`;
            const name = m.index === "default" ? "default" : m.name;
            return (
              <button
                key={m.index + m.name}
                data-row={id}
                className={`menu-row ${hot === id ? "hot" : ""}`}
                onMouseMove={() => hot !== id && onHot(id)}
                onClick={() => {
                  setCurrent(name);
                  onMic(name);
                }}
              >
                <span className="menu-check">
                  {same(name, current) && <Check size={14} strokeWidth={2.2} />}
                </span>
                <span className="menu-label">{m.name}</span>
              </button>
            );
          })}
        </div>
      )}
    </>
  );
};
