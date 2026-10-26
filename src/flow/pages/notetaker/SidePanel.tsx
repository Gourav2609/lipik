import { useEffect, useState, type ReactNode } from "react";
import { ChevronDown, SquarePen } from "lucide-react";
import { commands, type ChatThread } from "@/bindings";
import { IconButton } from "../../ui";
import { unwrap } from "../textflow";
import { Avatar, AvatarStack } from "./Avatars";
import {
  relativeDayName,
  shortDay,
  timeOfDay,
  timeRange,
  useMeetingDetail,
  type Meeting,
  type Note,
  type Person,
} from "./data";
import { meetingNames, Mentions } from "./names";

const SHOWN = 5;

const Header = ({
  title,
  subtitle,
  action,
}: {
  title: string;
  subtitle?: string;
  action?: ReactNode;
}) => (
  <div className="flex items-start gap-[8px] border-b border-[var(--fk-border)] pb-[16px]">
    <div className="min-w-0 flex-1">
      <h2 className="fk-display text-[20px] leading-[28px]">{title}</h2>
      {subtitle && (
        <p className="fk-num mt-[4px] text-[13px] leading-[18px] text-[var(--fk-text-muted)]">
          {subtitle}
        </p>
      )}
    </div>
    {action}
  </div>
);

const Section = ({
  label,
  children,
}: {
  label: string;
  children: ReactNode;
}) => (
  <div className="pt-[20px]">
    <div className="fk-eyebrow">{label}</div>
    {children}
  </div>
);

const Attendees = ({ people }: { people: Person[] }) => {
  const [all, setAll] = useState(false);
  const collapsible = people.length > SHOWN + 1;
  const shown = all || !collapsible ? people : people.slice(0, SHOWN);
  const rest = people.slice(SHOWN);
  return (
    <Section label="People">
      <ul className="mt-[8px]">
        {shown.map((p) => (
          <li key={p.id} className="flex h-[30px] items-center">
            <Avatar person={p} size={20} className="mr-[8px]" />
            <span className="truncate text-[14px]">
              {p.name}
              {p.isYou && (
                <span className="text-[var(--fk-text-muted)]"> (you)</span>
              )}
            </span>
          </li>
        ))}
      </ul>
      {collapsible && (
        <button
          type="button"
          aria-expanded={all}
          onClick={() => setAll((v) => !v)}
          className="flex h-[30px] items-center text-[14px] text-[var(--fk-text-muted)] hover:text-[var(--fk-text)]"
        >
          {all ? (
            "Show fewer"
          ) : (
            <>
              <AvatarStack
                people={rest}
                size={20}
                max={3}
                ring="var(--fk-surface)"
              />
              <span className="ml-[8px]">{rest.length} more</span>
            </>
          )}
          <ChevronDown
            size={14}
            strokeWidth={1.75}
            className={`ml-[4px] transition-transform duration-[var(--fk-t-base)] ${all ? "rotate-180" : ""}`}
          />
        </button>
      )}
    </Section>
  );
};

export const MeetingPanel = ({ meeting }: { meeting: Meeting }) => (
  <>
    <Header
      title={meeting.title}
      subtitle={`${relativeDayName(meeting.start)} · ${timeRange(meeting.start, meeting.end)}`}
    />
    <Attendees people={meeting.attendees} />
  </>
);

const overviewHint = (note: Note) =>
  note.summaryStatus === "unavailable" || note.summaryStatus === "failed"
    ? (note.summaryError ?? "This meeting has no summary.")
    : "The overview shows here once the notes are written.";

const Overview = ({ note }: { note: Note }) => {
  const { detail, loading } = useMeetingDetail(note.id);
  const overview = detail?.summary?.overview;
  return (
    <Section label="Overview">
      <p className="mt-[8px] text-[14.5px] leading-[24px]">
        {overview ? (
          <Mentions
            text={overview}
            names={meetingNames(detail)}
            className="text-[var(--fk-accent)]"
          />
        ) : loading ? null : (
          <span className="text-[var(--fk-text-muted)]">
            {overviewHint(note)}
          </span>
        )}
      </p>
    </Section>
  );
};

export const NotePanel = ({
  note,
  showOverview = true,
}: {
  note: Note;
  showOverview?: boolean;
}) => (
  <>
    <Header
      title={note.title}
      subtitle={`${relativeDayName(note.createdAt)} · ${timeOfDay(note.createdAt)}`}
    />
    {showOverview && <Overview note={note} />}
    {note.attendees.length > 0 ? (
      <Attendees people={note.attendees} />
    ) : (
      !showOverview && (
        <Section label="People">
          <p className="mt-[8px] text-[14px] leading-[20px] text-[var(--fk-text-muted)]">
            No one else was listed for this meeting.
          </p>
        </Section>
      )
    )}
  </>
);

export const ChatsPanel = ({
  threadId,
  version,
  onPick,
  onNew,
}: {
  threadId: string | null;
  version: number;
  onPick: (threadId: string) => void;
  onNew: () => void;
}) => {
  const [threads, setThreads] = useState<ChatThread[] | null>(null);
  useEffect(() => {
    let live = true;
    commands
      .notetakerChats(null)
      .then((r) => live && setThreads(unwrap(r)))
      .catch(() => live && setThreads([]));
    return () => {
      live = false;
    };
  }, [version]);
  return (
    <>
      <Header
        title="Earlier questions"
        action={
          <IconButton label="New chat" onClick={onNew}>
            <SquarePen size={16} strokeWidth={1.75} />
          </IconButton>
        }
      />
      {threads?.length === 0 && (
        <p className="pt-[16px] text-[14px] leading-[20px] text-[var(--fk-text-muted)]">
          Questions you ask about your meetings are kept here.
        </p>
      )}
      <div className="-mx-[8px] pt-[8px]">
        {threads?.map((t) => (
          <button
            type="button"
            key={t.thread_id}
            onClick={() => onPick(t.thread_id)}
            className={`flex w-full flex-col rounded-[8px] px-[12px] py-[8px] text-left transition-colors duration-[var(--fk-t-quick)] ${t.thread_id === threadId ? "bg-[var(--fk-menu-bg)] shadow-[var(--fk-elev-1)]" : "hover:bg-[var(--fk-surface-2)]"}`}
          >
            <span className="line-clamp-2 text-[14px] leading-[20px]">
              {t.first_question}
            </span>
            <span className="fk-num mt-[2px] text-[12px] text-[var(--fk-text-faint)]">
              {shortDay(t.updated_ms)} · {timeOfDay(t.updated_ms)}
            </span>
          </button>
        ))}
      </div>
    </>
  );
};

export const EmptyPanel = () => (
  <p className="text-[14px] leading-[20px] text-[var(--fk-text-muted)]">
    Pick a meeting or a note to see it here.
  </p>
);
