import { openUrl } from "@tauri-apps/plugin-opener";
import { CalendarDays, Cloud, X } from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";
import {
  commands,
  type CalendarAccess,
  type CalendarSource,
  type CalendarSourceKind,
} from "@/bindings";
import { APP_NAME } from "@/flow/brand";
import { Button, IconButton, LipikMark } from "../ui";
import { ModalShell } from "./ModalShell";
import type { OverlayProps } from "./types";

/** Window event fired after the calendar access or accounts may have changed. */
export const CALENDAR_CHANGED = "fk-calendar-changed";

const INTERNET_ACCOUNTS =
  "x-apple.systempreferences:com.apple.Internet-Accounts-Settings.extension";
const PRIVACY_CALENDARS =
  "x-apple.systempreferences:com.apple.preference.security?Privacy_Calendars";

const ACCOUNT_KINDS: CalendarSourceKind[] = [
  "google",
  "exchange",
  "icloud",
  "caldav",
];
const LISTED_KINDS: CalendarSourceKind[] = [...ACCOUNT_KINDS, "local"];

const BLOCKED_TEXT: Partial<Record<CalendarAccess, string>> = {
  denied: `Turn on ${APP_NAME} in System Settings › Privacy & Security › Calendars, then check again.`,
  write_only: `${APP_NAME} can only add events. Allow full access in System Settings › Privacy & Security › Calendars, then check again.`,
  restricted:
    "Calendar access is blocked on this Mac, for example by a device profile.",
  unsupported: "Calendar access isn't available on this computer.",
};

const sourceIcon = (kind: CalendarSourceKind) =>
  kind === "icloud" ? (
    <Cloud size={16} strokeWidth={1.75} />
  ) : (
    <CalendarDays size={16} strokeWidth={1.75} />
  );

const sourceName = (s: CalendarSource) =>
  s.kind === "local"
    ? "On My Mac"
    : s.kind === "exchange" && !s.title
      ? "Outlook"
      : s.title;

/** A sample meeting reminder, drawn as the popup Lipik shows above the pill. */
const Preview = () => (
  <div className="flex h-[148px] items-center justify-center rounded-[var(--fk-r-lg)] border-l-[3px] border-[var(--fk-accent)] bg-[var(--fk-accent-soft)]">
    <div
      aria-hidden
      className="flex w-[300px] items-center gap-[12px] rounded-[var(--fk-r-lg)] bg-[var(--fk-menu-bg)] p-[14px] shadow-[var(--fk-elev-2)]"
    >
      <LipikMark size={28} tile />
      <div className="min-w-0 flex-1">
        <div className="text-[14.5px] font-semibold leading-[20px]">
          Weekly sync
        </div>
        <div className="fk-num text-[12.5px] leading-[17px] text-[var(--fk-text-muted)]">
          Starts in 2 min
        </div>
      </div>
      <span className="flex h-[28px] items-center rounded-[var(--fk-r-sm)] bg-[var(--fk-accent)] px-[10px] text-[13px] font-semibold text-[var(--fk-accent-text)]">
        Take notes
      </span>
    </div>
  </div>
);

type View =
  | { kind: "choose"; busy: boolean }
  | { kind: "found"; sources: CalendarSource[] }
  | { kind: "none" }
  | { kind: "blocked"; access: CalendarAccess };

const loadView = async (access: CalendarAccess): Promise<View> => {
  if (access !== "full_access") return { kind: "blocked", access };
  const r = await commands.notetakerCalendarSources().catch(() => null);
  const sources = (r?.status === "ok" ? r.data : []).filter((s) =>
    LISTED_KINDS.includes(s.kind),
  );
  return sources.some((s) => ACCOUNT_KINDS.includes(s.kind))
    ? { kind: "found", sources }
    : { kind: "none" };
};

export default function ConnectCalendar({ onClose }: OverlayProps) {
  const [view, setView] = useState<View>({ kind: "choose", busy: false });

  useEffect(() => {
    let live = true;
    commands
      .notetakerCalendarAccess()
      .then(async (access) => {
        if (access === "not_determined") return;
        const next = await loadView(access);
        if (live) setView(next);
      })
      .catch(() => {});
    return () => {
      live = false;
    };
  }, []);

  const show = async (access: CalendarAccess) => {
    setView(await loadView(access));
    window.dispatchEvent(new Event(CALENDAR_CHANGED));
  };

  const connect = async () => {
    setView({ kind: "choose", busy: true });
    const r = await commands.notetakerRequestCalendarAccess().catch(() => null);
    await show(r?.status === "ok" ? r.data : "unsupported");
  };

  const checkAgain = async () =>
    show(
      await commands
        .notetakerCalendarAccess()
        .catch(() => "unsupported" as const),
    );

  const busy = view.kind === "choose" && view.busy;
  const title =
    view.kind === "found"
      ? "Calendar connected"
      : view.kind === "none"
        ? "No calendar accounts yet"
        : view.kind === "blocked"
          ? "Calendar access is off"
          : "Connect your calendar";
  const subtitle =
    view.kind === "found"
      ? `${APP_NAME} uses the calendars on this Mac.`
      : view.kind === "none"
        ? "Add your Google, Outlook or iCloud account in System Settings › Internet Accounts, with Calendars on. Then check again."
        : view.kind === "blocked"
          ? BLOCKED_TEXT[view.access]
          : `${APP_NAME} reads the Calendar app on this Mac to name your notes and remind you before meetings.`;

  let actions: ReactNode;
  if (view.kind === "choose") {
    actions = (
      <>
        <Button variant="secondary" onClick={onClose}>
          Not now
        </Button>
        <Button disabled={busy} onClick={connect}>
          Allow access
        </Button>
      </>
    );
  } else if (view.kind === "found") {
    actions = <Button onClick={onClose}>Done</Button>;
  } else {
    const canOpen =
      view.kind === "none" ||
      (view.access !== "unsupported" && view.access !== "restricted");
    actions = (
      <>
        <Button variant="secondary" onClick={() => void checkAgain()}>
          Check again
        </Button>
        {canOpen && (
          <Button
            onClick={() =>
              void openUrl(
                view.kind === "none" ? INTERNET_ACCOUNTS : PRIVACY_CALENDARS,
              ).catch(() => {})
            }
          >
            Open settings
          </Button>
        )}
      </>
    );
  }

  return (
    <ModalShell onClose={onClose} width={460} className="relative p-[24px]">
      <IconButton
        label="Close"
        onClick={onClose}
        className="absolute right-[12px] top-[12px]"
      >
        <X size={18} strokeWidth={1.75} />
      </IconButton>
      <div className="mt-[24px]">
        <Preview />
      </div>
      <h2 className="fk-display mt-[24px] text-[22px] leading-[30px]">
        {title}
      </h2>
      <p className="mt-[4px] text-[14.5px] leading-[21px] text-[var(--fk-text-muted)]">
        {subtitle}
      </p>
      {view.kind === "choose" && (
        <p className="mt-[8px] text-[13px] leading-[18px] text-[var(--fk-text-faint)]">
          Google, Outlook and iCloud accounts work once they're in the Calendar
          app.
        </p>
      )}
      {view.kind === "found" && (
        <div className="fk-scroll mt-[16px] max-h-[200px] border-t border-[var(--fk-border)]">
          {view.sources.map((s) => (
            <div
              key={s.id}
              className="flex h-[44px] items-center gap-[10px] border-b border-[var(--fk-border)] text-[14.5px]"
            >
              <span className="flex text-[var(--fk-text-muted)]">
                {sourceIcon(s.kind)}
              </span>
              <span className="min-w-0 flex-1 truncate font-medium">
                {sourceName(s)}
              </span>
              <span
                className="max-w-[200px] truncate text-[13px] text-[var(--fk-text-muted)]"
                title={s.calendars.join(", ")}
              >
                {s.calendars.length === 1
                  ? s.calendars[0]
                  : `${s.calendars.length} calendars`}
              </span>
            </div>
          ))}
        </div>
      )}
      <div className="mt-[24px] flex justify-end gap-[8px]">{actions}</div>
    </ModalShell>
  );
}
