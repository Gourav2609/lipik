import {
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { toast } from "sonner";
import {
  AppWindow,
  ChevronLeft,
  Copy,
  Ellipsis,
  Folder as FolderIcon,
  FolderPlus,
  RefreshCw,
} from "lucide-react";
import {
  commands,
  type MeetingDetail,
  type NoteSummary,
  type Segment,
} from "@/bindings";
import { Button, IconButton, SectionTabs } from "../../ui";
import {
  clockDuration,
  dismissHinglishError,
  openInWindow,
  redoNoteInHinglish,
  regenerateNote,
  renameNote,
  shortDay,
  timeOfDay,
  toNote,
  useMeetingDetail,
  wordDuration,
  type Folder,
  type Improving,
  type Processing,
} from "./data";
import { meetingNames, Mentions } from "./names";
import { SpeakerNameInput } from "@/components/notetaker/SpeakerNameInput";
import { NoteMenu, type NoteMenuMode } from "./NoteMenu";

export type NoteTab = "thoughts" | "transcript" | "summary";

const copy = (text: string) =>
  navigator.clipboard
    .writeText(text)
    .then(() => toast("Copied"))
    .catch(() => toast("Couldn't copy"));

/** A ruled line above each tab's content: a short label and the tab's tools. */
const Toolbar = ({
  label,
  children,
}: {
  label: ReactNode;
  children?: ReactNode;
}) => (
  <div className="flex h-[40px] items-center border-b border-[var(--fk-border)]">
    <span className="fk-num min-w-0 flex-1 truncate text-[13px] text-[var(--fk-text-muted)]">
      {label}
    </span>
    <span className="flex items-center gap-[4px]">{children}</span>
  </div>
);

const Tabs = ({
  value,
  onChange,
  live,
  writing,
}: {
  value: NoteTab;
  onChange: (tab: NoteTab) => void;
  live: boolean;
  writing: boolean;
}) => (
  <SectionTabs
    className="mt-[24px]"
    value={value}
    onChange={onChange}
    tabs={[
      {
        id: "summary",
        label: (
          <span className="flex items-center gap-[6px]">
            {writing && <span className="nt-ink-dot" aria-hidden />}
            Summary
          </span>
        ),
      },
      {
        id: "transcript",
        label: (
          <span className="flex items-center gap-[6px]">
            {live && <span className="nt-live-dot" aria-hidden />}
            Transcript
          </span>
        ),
      },
      { id: "thoughts", label: "My notes" },
    ]}
  />
);

const speakerLabel = (speaker: string) =>
  speaker === "Me" ? "You" : speaker === "Them" ? "Them" : speaker;

const groupBySpeaker = (segments: Segment[]) => {
  const groups: { speaker: string; key: string; items: Segment[] }[] = [];
  for (const s of segments) {
    const key = s.speaker_key || s.speaker;
    const last = groups[groups.length - 1];
    if (last && last.key === key) last.items.push(s);
    else groups.push({ speaker: s.speaker, key, items: [s] });
  }
  return groups;
};

/** Click a speaker's label to name them; the name goes on all their lines. */
const SpeakerLabel = ({
  meetingId,
  speaker,
  speakerKey,
  editable,
  onRenamed,
}: {
  meetingId: string;
  speaker: string;
  speakerKey: string;
  editable: boolean;
  onRenamed: () => void;
}) => {
  const [editing, setEditing] = useState(false);
  const style = `truncate text-[13px] font-semibold leading-[20px] ${speaker === "Me" ? "text-[var(--fk-accent)]" : ""}`;
  const done = async (name: string | null) => {
    setEditing(false);
    if (!name) return;
    const r = await commands
      .notetakerRenameSpeaker(meetingId, speakerKey, name)
      .catch(() => null);
    if (r?.status !== "ok")
      toast(`Couldn't rename: ${r?.error ?? "try again"}`);
    onRenamed();
  };
  if (editing)
    return (
      <SpeakerNameInput
        speaker={speaker}
        placeholder={speakerLabel(speaker)}
        onDone={(name) => void done(name)}
        className={`${style} -mx-[4px] w-full rounded-[6px] bg-[var(--fk-surface)] px-[4px] outline-none`}
      />
    );
  if (!editable) return <div className={style}>{speakerLabel(speaker)}</div>;
  return (
    <button
      type="button"
      title="Name this speaker"
      onClick={() => setEditing(true)}
      className={`${style} -mx-[4px] block max-w-full cursor-text rounded-[6px] px-[4px] text-left hover:bg-[var(--fk-surface)]`}
    >
      {speakerLabel(speaker)}
    </button>
  );
};

const transcriptText = (segments: Segment[]) =>
  segments.map((s) => `${speakerLabel(s.speaker)}: ${s.text}`).join("\n");

const Transcript = ({
  detail,
  live,
  writing,
  focusMs,
  onRenamed,
}: {
  detail: MeetingDetail;
  live: boolean;
  /** Notes are being written: speakers can't be renamed until they are. */
  writing: boolean;
  focusMs: number | null;
  onRenamed: () => void;
}) => {
  const { segments, meeting } = detail;
  const list = useRef<HTMLDivElement>(null);
  const [flash, setFlash] = useState<number | null>(null);
  const [, tick] = useState(0);
  const speakers = new Set(segments.map((s) => s.speaker)).size;
  const length = (meeting.end_ms ?? Date.now()) - meeting.start_ms;

  useEffect(() => {
    if (!live) return;
    const id = window.setInterval(() => tick((n) => n + 1), 1000);
    return () => window.clearInterval(id);
  }, [live]);

  useEffect(() => {
    if (focusMs == null || !segments.length) return;
    const target =
      [...segments].reverse().find((s) => s.start_ms <= focusMs) ?? segments[0];
    setFlash(target.start_ms);
    list.current
      ?.querySelector(`[data-start="${target.start_ms}"]`)
      ?.scrollIntoView({ block: "center" });
    const id = window.setTimeout(() => setFlash(null), 2000);
    return () => window.clearTimeout(id);
  }, [focusMs, segments.length > 0]);

  return (
    <div>
      <Toolbar
        label={`${speakers} ${speakers === 1 ? "speaker" : "speakers"} · ${clockDuration(length)}`}
      >
        <IconButton
          label="Copy transcript"
          onClick={() => void copy(transcriptText(segments))}
        >
          <Copy size={16} strokeWidth={1.75} />
        </IconButton>
      </Toolbar>
      {segments.length === 0 ? (
        <p className="flex items-center gap-[8px] pt-[24px] text-[15px] text-[var(--fk-text-muted)]">
          {live && <span className="nt-live-dot" aria-hidden />}
          {live ? "Listening…" : "Nothing was transcribed in this meeting."}
        </p>
      ) : (
        <div ref={list}>
          {groupBySpeaker(segments).map((g, i) => (
            <div
              key={`${g.items[0].start_ms}-${i}`}
              className="grid grid-cols-[96px_minmax(0,1fr)] gap-x-[16px] border-b border-[var(--fk-border)] py-[12px]"
            >
              <div className="min-w-0 pt-[2px]">
                <SpeakerLabel
                  meetingId={meeting.id}
                  speaker={g.speaker}
                  speakerKey={g.key}
                  editable={!live && !writing && g.key !== "me"}
                  onRenamed={onRenamed}
                />
                <div className="fk-num text-[12px] leading-[16px] text-[var(--fk-text-faint)]">
                  {clockDuration(g.items[0].start_ms)}
                </div>
              </div>
              <div className="flex flex-col gap-[4px]">
                {g.items.map((s) => (
                  <p
                    key={s.start_ms}
                    data-start={s.start_ms}
                    className={`-mx-[8px] rounded-[8px] px-[8px] text-[15px] leading-[24px] ${flash === s.start_ms ? "nt-flash" : ""}`}
                  >
                    {s.text}
                  </p>
                ))}
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
};

const readMinutes = (s: NoteSummary) => {
  const words = [
    s.overview,
    ...s.sections.flatMap((x) => [x.heading ?? "", ...(x.bullets ?? [])]),
    ...s.key_points,
    ...s.decisions,
    ...s.action_items.map((a) => a.text),
  ]
    .join(" ")
    .split(/\s+/)
    .filter(Boolean).length;
  return Math.max(1, Math.round(words / 200));
};

const summaryText = (title: string, s: NoteSummary) =>
  [
    `# ${title}`,
    "",
    s.overview,
    ...s.sections.flatMap((x) => [
      "",
      `## ${x.heading ?? ""}`,
      ...(x.bullets ?? []).map((b) => `- ${b}`),
    ]),
    ...(s.key_points.length
      ? ["", "## Key points", ...s.key_points.map((b) => `- ${b}`)]
      : []),
    ...(s.decisions.length
      ? ["", "## Decisions", ...s.decisions.map((b) => `- ${b}`)]
      : []),
    ...(s.action_items.length
      ? [
          "",
          "## Action items",
          ...s.action_items.map((a) => `- ${a.text}${actionMeta(a)}`),
        ]
      : []),
  ].join("\n");

const actionMeta = (a: { owner: string | null; due: string | null }) => {
  const meta = [a.owner, a.due].filter(Boolean).join(", ");
  return meta ? ` (${meta})` : "";
};

const Bullets = ({
  heading,
  items,
}: {
  heading?: string;
  items: ReactNode[];
}) => (
  <div className="mt-[24px]">
    {heading && (
      <h3 className="text-[16px] font-semibold leading-[22px]">{heading}</h3>
    )}
    <ul className="mt-[8px]">
      {items.map((item, i) => (
        <li key={i} className="relative pl-[20px] text-[15px] leading-[26px]">
          <span className="absolute left-0 top-[12px] h-[1.5px] w-[8px] rounded-full bg-[var(--fk-text-faint)]" />
          {item}
        </li>
      ))}
    </ul>
  </div>
);

const STEPS = [
  "Saving the audio",
  "Writing the final transcript",
  "Finding decisions and next steps",
];

const ProgressCard = ({ processing }: { processing: Processing | null }) => {
  const transcribing = processing?.stage === "transcribing";
  const step = !processing ? 0 : processing.stage === "summarizing" ? 2 : 1;
  const percent =
    transcribing && processing.progress > 0
      ? Math.round(processing.progress * 100)
      : null;
  return (
    <div>
      <div className="rounded-[14px] bg-[var(--fk-surface)] px-[20px] py-[16px]">
        <div className="flex items-baseline gap-[12px]">
          <span
            key={step}
            className="nt-fade min-w-0 flex-1 text-[15px] leading-[22px]"
          >
            {STEPS[step]}
          </span>
          <span className="fk-num shrink-0 text-[13px] text-[var(--fk-text-muted)]">
            {percent !== null && `${percent}% · `}Step {step + 1} of{" "}
            {STEPS.length}
          </span>
        </div>
        <div
          className="nt-progress mt-[12px]"
          data-busy={percent === null || undefined}
        >
          <i style={percent === null ? undefined : { width: `${percent}%` }} />
        </div>
      </div>
      {[0, 1].map((g) => (
        <div key={g} className="mt-[24px] flex flex-col gap-[10px]">
          <span className="nt-skeleton h-[12px] w-[200px] rounded-[4px]" />
          <span className="nt-skeleton h-[10px] w-full rounded-[4px]" />
          <span className="nt-skeleton h-[10px] w-[80%] rounded-[4px]" />
          <span className="nt-skeleton h-[10px] w-[88%] rounded-[4px]" />
        </div>
      ))}
    </div>
  );
};

const Summary = ({
  detail,
  processing,
  onRegenerate,
}: {
  detail: MeetingDetail;
  processing: Processing | null;
  onRegenerate: () => void;
}) => {
  const { meeting, summary } = detail;
  if (meeting.status === "recording")
    return (
      <p className="text-[15px] leading-[22px] text-[var(--fk-text-muted)]">
        The summary is written when the meeting ends.
      </p>
    );
  if (
    processing ||
    meeting.status === "processing" ||
    meeting.summary_status === "pending"
  )
    return <ProgressCard processing={processing} />;
  if (!summary)
    return (
      <div className="flex items-center gap-[16px] rounded-[14px] bg-[var(--fk-surface)] px-[20px] py-[16px]">
        <p className="min-w-0 flex-1 text-[15px] leading-[22px]">
          {meeting.summary_error ?? "This meeting has no summary yet."}
        </p>
        <Button
          variant="secondary"
          size="sm"
          icon={<RefreshCw size={14} strokeWidth={1.75} />}
          onClick={onRegenerate}
        >
          Try again
        </Button>
      </div>
    );
  const names = meetingNames(detail);
  const mention = (text: string) => (
    <Mentions text={text} names={names} className="text-[var(--fk-accent)]" />
  );
  return (
    <div>
      <Toolbar label={`${readMinutes(summary)} min read`}>
        <IconButton
          label="Copy summary"
          onClick={() => void copy(summaryText(meeting.title, summary))}
        >
          <Copy size={16} strokeWidth={1.75} />
        </IconButton>
      </Toolbar>
      <p className="mt-[20px] text-[15px] leading-[26px]">
        {mention(summary.overview)}
      </p>
      {summary.sections.map((s, i) => (
        <Bullets
          key={i}
          heading={s.heading}
          items={(s.bullets ?? []).map(mention)}
        />
      ))}
      {summary.key_points.length > 0 && (
        <Bullets heading="Key points" items={summary.key_points.map(mention)} />
      )}
      {summary.decisions.length > 0 && (
        <Bullets heading="Decisions" items={summary.decisions.map(mention)} />
      )}
      {summary.action_items.length > 0 && (
        <Bullets
          heading="Next steps"
          items={summary.action_items.map((a) => (
            <>
              {mention(a.text)}
              <span className="text-[var(--fk-text-muted)]">
                {actionMeta(a)}
              </span>
            </>
          ))}
        />
      )}
    </div>
  );
};

const SAVE_DELAY_MS = 600;

const Hint = ({
  text,
  action,
  onAction,
}: {
  text: string;
  action: string;
  onAction: () => void;
}) => (
  <p className="mb-[12px] flex items-center gap-[12px] rounded-[10px] border-l-[3px] border-[var(--fk-accent)] bg-[var(--fk-accent-soft)] py-[8px] pl-[12px] pr-[8px] text-[14px] leading-[20px]">
    <span className="min-w-0 flex-1">{text}</span>
    <Button variant="ghost" size="sm" onClick={onAction}>
      {action}
    </Button>
  </p>
);

const Thoughts = ({
  detail,
  live,
  onShowSummary,
}: {
  detail: MeetingDetail;
  live: boolean;
  onShowSummary: () => void;
}) => {
  const id = detail.meeting.id;
  const [text, setText] = useState<string | null>(null);
  const unsaved = useRef<string | null>(null);
  const saving = useRef(0);
  const area = useRef<HTMLTextAreaElement>(null);

  const save = () => {
    const markdown = unsaved.current;
    if (markdown == null) return;
    unsaved.current = null;
    saving.current += 1;
    void commands
      .notetakerSaveThoughts(id, markdown)
      .then((r) => {
        if (r.status === "error") toast(`Couldn't save your notes: ${r.error}`);
      })
      .finally(() => (saving.current -= 1));
  };

  useEffect(() => {
    const load = () => {
      if (unsaved.current != null || saving.current > 0) return;
      void commands.notetakerGet(id).then((r) => {
        if (unsaved.current != null || saving.current > 0) return;
        setText(
          r.status === "ok" && r.data ? r.data.thoughts : detail.thoughts,
        );
      });
    };
    load();
    window.addEventListener("focus", load);
    return () => window.removeEventListener("focus", load);
  }, [id]);

  useEffect(() => {
    if (unsaved.current == null) return;
    const t = window.setTimeout(save, SAVE_DELAY_MS);
    return () => window.clearTimeout(t);
  }, [text]);

  useEffect(() => save, []);

  useLayoutEffect(() => {
    const el = area.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${el.scrollHeight}px`;
  }, [text]);

  return (
    <div>
      {live && (
        <Hint
          text="Recording now. Write your notes in the notepad window."
          action="Open notepad"
          onAction={() => void openInWindow(id)}
        />
      )}
      {!live && text === "" && detail.summary && (
        <Hint
          text="Your summary is ready."
          action="Read it"
          onAction={onShowSummary}
        />
      )}
      <textarea
        ref={area}
        value={text ?? ""}
        readOnly={live || text == null}
        onChange={(e) => {
          unsaved.current = e.target.value;
          setText(e.target.value);
        }}
        onBlur={save}
        rows={8}
        placeholder={
          live
            ? ""
            : "Write your own notes. They stay separate from the summary."
        }
        aria-label="My notes"
        className="nt-ruled-paper w-full resize-none bg-transparent pt-[1px] text-[15px] outline-none placeholder:text-[var(--fk-text-faint)]"
      />
    </div>
  );
};

const defaultTab = (detail: MeetingDetail): NoteTab =>
  detail.meeting.status === "recording" ? "transcript" : "summary";

export const NoteView = ({
  id,
  folders,
  initialTab,
  focusMs,
  onBack,
  onChanged,
  onDeleted,
}: {
  id: string;
  folders: Folder[];
  initialTab: NoteTab | null;
  focusMs: number | null;
  onBack: () => void;
  onChanged: () => void;
  onDeleted: () => void;
}) => {
  const { detail, loading, processing, improving, reload } =
    useMeetingDetail(id);
  const [tab, setTab] = useState<NoteTab | null>(initialTab);
  const [title, setTitle] = useState<string | null>(null);
  const [menu, setMenu] = useState<{
    rect: DOMRect;
    mode: NoteMenuMode;
  } | null>(null);

  useEffect(() => {
    if (initialTab) setTab(initialTab);
  }, [initialTab, focusMs]);

  if (!detail)
    return (
      <div>
        <BackButton onBack={onBack} />
        {!loading && (
          <p className="pt-[24px] text-[15px] text-[var(--fk-text-muted)]">
            This note was deleted.
          </p>
        )}
      </div>
    );

  const { meeting } = detail;
  const note = toNote(meeting);
  const shownTab = tab ?? defaultTab(detail);
  const live = meeting.status === "recording";
  const writing =
    !!processing ||
    meeting.status === "processing" ||
    (meeting.summary_status === "pending" && !live);

  const changed = () => {
    reload();
    onChanged();
  };

  const commitTitle = async () => {
    const next = title?.trim();
    setTitle(null);
    if (!next || next === meeting.title) return;
    if (await renameNote(meeting.id, next)) changed();
  };

  const regenerate = async () => {
    if (await regenerateNote(meeting.id)) changed();
  };

  const redoInHinglish = async () => {
    if (await redoNoteInHinglish(meeting.id)) changed();
  };

  const dismissRedoError = async () => {
    if (await dismissHinglishError(meeting.id)) changed();
  };

  return (
    <div>
      <div className="flex h-[30px] items-center justify-between">
        <BackButton onBack={onBack} />
        <div className="flex items-center gap-[4px]">
          <Button
            variant="secondary"
            size="sm"
            icon={<AppWindow size={14} strokeWidth={1.75} />}
            onClick={() => void openInWindow(meeting.id)}
          >
            Open in window
          </Button>
          <IconButton
            label="Note options"
            aria-expanded={!!menu}
            onClick={(e) =>
              setMenu(
                menu
                  ? null
                  : {
                      rect: e.currentTarget.getBoundingClientRect(),
                      mode: "main",
                    },
              )
            }
          >
            <Ellipsis size={16} strokeWidth={1.75} />
          </IconButton>
        </div>
      </div>
      <input
        value={title ?? meeting.title}
        onChange={(e) => setTitle(e.target.value)}
        onBlur={() => void commitTitle()}
        onKeyDown={(e) => {
          if (e.key === "Enter") e.currentTarget.blur();
          if (e.key === "Escape") {
            const input = e.currentTarget;
            setTitle(null);
            requestAnimationFrame(() => input.blur());
          }
        }}
        placeholder="Untitled note"
        aria-label="Title"
        className="fk-display mt-[20px] w-full bg-transparent text-[28px] leading-[38px] text-[var(--fk-text)] outline-none placeholder:text-[var(--fk-text-faint)]"
      />
      <div className="fk-num mt-[4px] flex flex-wrap items-center gap-x-[8px] text-[13px] leading-[30px] text-[var(--fk-text-muted)]">
        <span>
          {shortDay(meeting.start_ms)} · {timeOfDay(meeting.start_ms)}
        </span>
        {meeting.end_ms != null && (
          <>
            <span aria-hidden>·</span>
            <span>{wordDuration(meeting.end_ms - meeting.start_ms)}</span>
          </>
        )}
        <span aria-hidden>·</span>
        <button
          type="button"
          onClick={(e) =>
            setMenu({
              rect: e.currentTarget.getBoundingClientRect(),
              mode: "folder",
            })
          }
          className={`-mx-[6px] flex h-[26px] items-center gap-[6px] rounded-[8px] px-[6px] transition-colors duration-[var(--fk-t-quick)] hover:bg-[var(--fk-hover)] hover:text-[var(--fk-text)] ${meeting.folder ? "" : "text-[var(--fk-accent)]"}`}
        >
          {meeting.folder ? (
            <FolderIcon size={14} strokeWidth={1.75} />
          ) : (
            <FolderPlus size={14} strokeWidth={1.75} />
          )}
          {meeting.folder ?? "Add to folder"}
        </button>
      </div>
      <Tabs value={shownTab} onChange={setTab} live={live} writing={writing} />
      {improving || meeting.hinglish_redo ? (
        <p className="pt-[12px] text-[13px] leading-[18px] text-[var(--fk-text-muted)]">
          {redoStatus(improving, meeting.hinglish_redo === "asked")}
        </p>
      ) : (
        meeting.hinglish_error && (
          <div className="flex items-center gap-[12px] pt-[12px]">
            <p className="min-w-0 flex-1 text-[13px] leading-[18px]">
              Couldn't redo the transcript in Hinglish: {meeting.hinglish_error}
            </p>
            <Button
              variant="ghost"
              size="sm"
              onClick={() => void dismissRedoError()}
            >
              Dismiss
            </Button>
            {meeting.has_audio && (
              <Button
                variant="secondary"
                size="sm"
                icon={<RefreshCw size={14} strokeWidth={1.75} />}
                onClick={() => void redoInHinglish()}
              >
                Try again
              </Button>
            )}
          </div>
        )
      )}
      <div key={shownTab} className="nt-fade pt-[16px]">
        {shownTab === "transcript" && (
          <Transcript
            detail={detail}
            live={live}
            writing={writing}
            focusMs={focusMs}
            onRenamed={reload}
          />
        )}
        {shownTab === "summary" && (
          <Summary
            detail={detail}
            processing={processing}
            onRegenerate={() => void regenerate()}
          />
        )}
        {shownTab === "thoughts" && (
          <Thoughts
            key={meeting.id}
            detail={detail}
            live={live}
            onShowSummary={() => setTab("summary")}
          />
        )}
      </div>
      {menu && (
        <NoteMenu
          note={note}
          folders={folders}
          anchor={menu.rect}
          initialMode={menu.mode}
          onClose={() => setMenu(null)}
          onChanged={changed}
          onDeleted={onDeleted}
        />
      )}
    </div>
  );
};

/** The status line of a Hinglish redo, with how far it got when known. */
const redoStatus = (improving: Improving | null, asked: boolean) => {
  const percent =
    improving && improving.progress > 0
      ? ` ${Math.min(99, Math.round(improving.progress * 100))}%`
      : "";
  if (improving?.stage === "downloading")
    return `Getting the Hinglish speech model…${percent}`;
  return asked
    ? `Redoing the transcript in Hinglish…${percent}`
    : `Hindi heard: improving the transcript…${percent}`;
};

const BackButton = ({ onBack }: { onBack: () => void }) => (
  <Button
    variant="ghost"
    size="sm"
    className="-ml-[8px] text-[var(--fk-text-muted)]"
    icon={<ChevronLeft size={16} strokeWidth={1.75} />}
    onClick={onBack}
  >
    Back
  </Button>
);
