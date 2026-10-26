import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { CalendarDays } from "lucide-react";
import { XGlyph } from "./parts";
import {
  calendarConnected,
  openMainPage,
  upcomingMeetings,
  type Meeting,
} from "./backend";

// "Upcoming meetings" (notetaker1 49.47-56.40, NOTETAKER_UI.md §3): a 422 x 66 glass
// capsule centred on the record button, 10.5 pt beside the chevron tab. Without a
// calendar it becomes the 423 x 158 "No upcoming meetings" panel in one frame.
export const MEETINGS = { w: 422, h: 66, gap: 10.5, row: 28, pad: 18.5 };
export const NO_MEETINGS = { w: 423, h: 158 };
/** The close button sits on the panel's corner and reaches this far outside it. */
export const MEETINGS_OUT = 10;
/** The capsule turned into the panel 1.16 s after it opened. */
const SWAP_MS = 1160;

const sizeFor = (n: number, connected: boolean) =>
  n
    ? {
        w: MEETINGS.w,
        h: Math.max(MEETINGS.h, 2 * MEETINGS.pad + n * MEETINGS.row),
      }
    : connected
      ? MEETINGS
      : NO_MEETINGS;

type Props = {
  place: (w: number, h: number) => React.CSSProperties;
  closing: boolean;
  onSize: (w: number, h: number) => void;
  onClose: () => void;
};

export const Meetings: React.FC<Props> = ({
  place,
  closing,
  onSize,
  onClose,
}) => {
  const { t: tr } = useTranslation();
  const [meetings, setMeetings] = useState<Meeting[]>([]);
  const [connected, setConnected] = useState(true);

  useEffect(() => {
    let live = true;
    const opened = performance.now();
    calendarConnected().then(async (ok) => {
      if (ok) {
        const m = await upcomingMeetings();
        if (live) setMeetings(m.slice(0, 4));
        return;
      }
      const wait = Math.max(0, SWAP_MS - (performance.now() - opened));
      window.setTimeout(() => live && setConnected(false), wait);
    });
    return () => {
      live = false;
    };
  }, []);

  const { w, h } = sizeFor(meetings.length, connected);
  useEffect(() => onSize(w, h), [w, h, onSize]);

  const connect = () => {
    onClose();
    openMainPage("connect-calendar");
  };

  if (!connected && meetings.length === 0)
    return (
      <div
        className={`pill-meetings none ${closing ? "out" : ""}`}
        style={place(w, h)}
      >
        <button className="meetings-close" onClick={onClose}>
          <XGlyph size={8} stroke={1.3} />
        </button>
        <CalendarDays
          className="meetings-cal"
          size={24}
          strokeWidth={1.5}
          absoluteStrokeWidth
        />
        <span className="meetings-none">
          {tr("pill.noUpcoming", "No upcoming meetings")}
        </span>
        <button className="meetings-connect" onClick={connect}>
          {tr("pill.connectCalendar", "Connect calendar")}
        </button>
      </div>
    );

  return (
    <div
      className={`pill-meetings ${closing ? "out" : ""}`}
      style={place(w, h)}
    >
      {meetings.length === 0 ? (
        <span className="meetings-empty">
          {tr("pill.allDone", "You're all done for today!")}
        </span>
      ) : (
        meetings.map((m) => (
          <div key={m.id} className="meetings-row">
            <span className="meetings-time">{m.at}</span>
            <span className="meetings-title">{m.title}</span>
          </div>
        ))
      )}
    </div>
  );
};
