import { useState } from "react";
import {
  CalendarDays,
  ChevronDown,
  Ellipsis,
  RefreshCw,
  Video,
} from "lucide-react";
import type { CalendarAccess } from "@/bindings";
import { Popover } from "../../overlays/Popover";
import { Button, IconButton } from "../../ui";
import { AvatarStack } from "./Avatars";
import {
  groupByDay,
  joinMeeting,
  relativeDayLabel,
  startOfDay,
  timeRange,
  type JoinMode,
  type Meeting,
} from "./data";
import { MenuItem } from "./NoteMenu";

const COLLAPSED_COUNT = 3;

const JoinButton = ({
  meeting,
  onMenu,
}: {
  meeting: Meeting;
  onMenu: (open: boolean) => void;
}) => {
  const [menu, setMenu] = useState<DOMRect | null>(null);
  const toggle = (rect: DOMRect | null) => {
    setMenu(rect);
    onMenu(!!rect);
  };
  const join = (mode: JoinMode) => {
    toggle(null);
    void joinMeeting(meeting, mode);
  };
  return (
    <div
      className="flex h-[30px] shrink-0 overflow-hidden rounded-[8px] bg-[var(--fk-accent)] text-[var(--fk-accent-text)]"
      onClick={(e) => e.stopPropagation()}
    >
      <button
        type="button"
        onClick={() => join("join-and-start")}
        className="flex items-center gap-[6px] pl-[10px] pr-[10px] text-[13.5px] font-semibold transition-colors duration-[var(--fk-t-quick)] hover:bg-[var(--fk-accent-hover)]"
      >
        <Video size={16} strokeWidth={1.75} />
        {meeting.joinUrl ? "Join and record" : "Record"}
      </button>
      <span className="my-[7px] w-px bg-[var(--fk-accent-text)] opacity-30" />
      <button
        type="button"
        aria-label="More ways to join"
        aria-expanded={!!menu}
        onClick={(e) =>
          toggle(
            menu
              ? null
              : e.currentTarget.parentElement!.getBoundingClientRect(),
          )
        }
        className="flex w-[28px] items-center justify-center transition-colors duration-[var(--fk-t-quick)] hover:bg-[var(--fk-accent-hover)]"
      >
        <ChevronDown size={14} strokeWidth={1.75} />
      </button>
      {menu && (
        <Popover
          onClose={() => toggle(null)}
          className="p-[4px]"
          style={{ top: menu.bottom + 4, left: menu.right - 200, width: 200 }}
        >
          <MenuItem
            disabled={!meeting.joinUrl}
            onClick={() => join("join-only")}
          >
            Join without recording
          </MenuItem>
          <MenuItem onClick={() => join("start-only")}>Record only</MenuItem>
        </Popover>
      )}
    </div>
  );
};

const OptionsButton = ({
  meeting,
  onMenu,
}: {
  meeting: Meeting;
  onMenu: (open: boolean) => void;
}) => {
  const [menu, setMenu] = useState<DOMRect | null>(null);
  const toggle = (rect: DOMRect | null) => {
    setMenu(rect);
    onMenu(!!rect);
  };
  const copyLink = () => {
    toggle(null);
    if (meeting.joinUrl) void navigator.clipboard.writeText(meeting.joinUrl);
  };
  return (
    <div className="mr-[8px]" onClick={(e) => e.stopPropagation()}>
      <IconButton
        label="Meeting options"
        aria-expanded={!!menu}
        onClick={(e) =>
          toggle(menu ? null : e.currentTarget.getBoundingClientRect())
        }
      >
        <Ellipsis size={16} strokeWidth={1.75} />
      </IconButton>
      {menu && (
        <Popover
          onClose={() => toggle(null)}
          className="p-[4px]"
          style={{ top: menu.bottom + 4, left: menu.right - 200, width: 200 }}
        >
          <MenuItem disabled={!meeting.joinUrl} onClick={copyLink}>
            Copy meeting link
          </MenuItem>
        </Popover>
      )}
    </div>
  );
};

const MeetingRow = ({
  meeting,
  selected,
  onSelect,
}: {
  meeting: Meeting;
  selected: boolean;
  onSelect: () => void;
}) => {
  const [hover, setHover] = useState(false);
  const [focused, setFocused] = useState(false);
  const [menuOpen, setMenuOpen] = useState(false);
  const showOptions = hover || menuOpen;
  // A closing menu removes the focused element without a blur event.
  const onMenu = (open: boolean) => {
    setMenuOpen(open);
    if (!open) setFocused(false);
  };
  const active = selected || hover || focused || menuOpen;
  return (
    <div
      role="button"
      tabIndex={0}
      onClick={onSelect}
      onKeyDown={(e) => {
        if (
          e.target !== e.currentTarget ||
          (e.key !== "Enter" && e.key !== " ")
        )
          return;
        e.preventDefault();
        onSelect();
      }}
      onMouseEnter={() => setHover(true)}
      onMouseLeave={() => setHover(false)}
      onFocus={() => setFocused(true)}
      onBlur={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget)) setFocused(false);
      }}
      data-selected={selected || undefined}
      data-open={menuOpen || undefined}
      className="nt-line flex h-[52px] items-center pl-[12px] pr-[8px]"
    >
      <span className="fk-num w-[148px] shrink-0 whitespace-nowrap text-[13px] text-[var(--fk-text-muted)]">
        {timeRange(meeting.start, meeting.end)}
      </span>
      {meeting.calendarColor && (
        <span
          className="mr-[8px] h-[8px] w-[8px] shrink-0 rounded-full"
          style={{ background: meeting.calendarColor }}
        />
      )}
      <span className="min-w-0 flex-1 truncate text-[15px] leading-[22px]">
        {meeting.title}
      </span>
      {!active && (
        <span className="ml-[12px]">
          <AvatarStack people={meeting.attendees} size={20} />
        </span>
      )}
      {showOptions && <OptionsButton meeting={meeting} onMenu={onMenu} />}
      {active && <JoinButton meeting={meeting} onMenu={onMenu} />}
    </div>
  );
};

const DayLabel = ({ label }: { label: string }) => (
  <div className="fk-eyebrow flex h-[32px] items-end pb-[8px] pl-[12px]">
    {label}
  </div>
);

const SectionHead = ({ onRefresh }: { onRefresh?: () => void }) => (
  <div className="mb-[8px] flex h-[30px] items-center justify-between">
    <h2 className="text-[16px] font-semibold leading-[22px]">Upcoming</h2>
    {onRefresh && (
      <IconButton label="Refresh calendar" onClick={onRefresh}>
        <RefreshCw size={16} strokeWidth={1.75} />
      </IconButton>
    )}
  </div>
);

// The Connect calendar dialog explains the other access states.
const ConnectCalendar = ({
  access,
  onConnect,
}: {
  access: CalendarAccess;
  onConnect: () => void;
}) => (
  <section>
    <SectionHead />
    <div className="flex items-center gap-[16px] rounded-[14px] bg-[var(--fk-surface)] px-[20px] py-[16px]">
      <CalendarDays
        size={18}
        strokeWidth={1.75}
        className="shrink-0 text-[var(--fk-text-muted)]"
      />
      <div className="min-w-0 flex-1">
        <p className="text-[15px] font-semibold leading-[22px]">
          See your meetings here
        </p>
        <p className="text-[13px] leading-[18px] text-[var(--fk-text-muted)]">
          {access === "restricted" || access === "unsupported"
            ? "Calendar access isn't available on this Mac."
            : "Connect your calendar to join and record a call in one click."}
        </p>
      </div>
      {access !== "restricted" && access !== "unsupported" && (
        <Button variant="secondary" size="sm" onClick={onConnect}>
          Connect calendar
        </Button>
      )}
    </div>
  </section>
);

export const MeetingsCard = ({
  meetings,
  loading,
  access,
  showToday,
  finishedToday,
  selectedId,
  onSelect,
  onRefresh,
  onConnect,
}: {
  meetings: Meeting[];
  loading: boolean;
  access: CalendarAccess;
  showToday: boolean;
  finishedToday: number;
  selectedId: string | null;
  onSelect: (id: string) => void;
  onRefresh: () => void;
  onConnect: () => void;
}) => {
  const [expanded, setExpanded] = useState(false);
  if (access !== "full_access" && !loading)
    return <ConnectCalendar access={access} onConnect={onConnect} />;
  const today = startOfDay(Date.now());
  const visible = expanded ? meetings : meetings.slice(0, COLLAPSED_COUNT);
  const groups = groupByDay(visible, (m) => m.start);
  const hasToday = groups[0]?.day === today;
  const sections = [
    ...(showToday && !hasToday ? [{ day: today, items: [] as Meeting[] }] : []),
    ...groups,
  ];

  return (
    <section>
      <SectionHead onRefresh={onRefresh} />
      {sections.map((section, i) => (
        <div key={section.day} className={i ? "mt-[12px]" : ""}>
          <DayLabel label={relativeDayLabel(section.day)} />
          <div className="nt-ruled-top">
            {section.items.length === 0 ? (
              <p className="flex h-[52px] items-center border-b border-[var(--fk-border)] pl-[12px] text-[14px] text-[var(--fk-text-muted)]">
                {loading ? null : (
                  <span className="nt-appear">
                    {finishedToday
                      ? `${finishedToday} ${finishedToday === 1 ? "meeting" : "meetings"} recorded today.`
                      : "No meetings today."}
                  </span>
                )}
              </p>
            ) : (
              section.items.map((m) => (
                <MeetingRow
                  key={m.id}
                  meeting={m}
                  selected={m.id === selectedId}
                  onSelect={() => onSelect(m.id)}
                />
              ))
            )}
          </div>
        </div>
      ))}
      {meetings.length > COLLAPSED_COUNT && (
        <Button
          variant="ghost"
          size="sm"
          className="mt-[8px] text-[var(--fk-text-muted)]"
          onClick={() => setExpanded((v) => !v)}
        >
          {expanded ? "Show fewer" : `Show all ${meetings.length}`}
          <ChevronDown
            size={14}
            strokeWidth={1.75}
            className={`transition-transform duration-[var(--fk-t-base)] ${expanded ? "rotate-180" : ""}`}
          />
        </Button>
      )}
    </section>
  );
};
