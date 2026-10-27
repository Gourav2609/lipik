import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { Citation } from "@/bindings";
import { AskDrawer, BottomBar, useAsk } from "./Ask";
import { Header } from "./Header";
import { LiveNotices, SummaryNotices, ThoughtsNote } from "./Notices";
import { SummaryTab, isGenerating } from "./SummaryTab";
import { Tabs, type Tab } from "./Tabs";
import { ThoughtsTab } from "./ThoughtsTab";
import { TopBar } from "./TopBar";
import { TranscriptTab } from "./TranscriptTab";
import {
  copyText,
  meetingLength,
  noteMarkdown,
  peopleNames,
  summaryMarkdown,
  transcriptText,
} from "./format";
import { useNotepad } from "./useNotepad";

const NOTICE_MS = 2200;
const HELP_MS = 7000;
const TOAST_MS = 2400;

export const Notepad = () => {
  const np = useNotepad();
  const { detail, step, reveal, actions } = np;
  const meeting = detail?.meeting ?? null;
  const live = meeting?.status === "recording";
  const generating = !!detail && isGenerating(detail);

  const [tab, setTab] = useState<Tab>("transcript");
  const [notice, setNotice] = useState<string | null>(null);
  const [help, setHelp] = useState<string | null>(null);
  const [focusMs, setFocusMs] = useState<{ ms: number; at: number } | null>(
    null,
  );
  const scrollRef = useRef<HTMLDivElement>(null);
  const atBottom = useRef(true);
  const tabFor = useRef<string | null>(null);

  const ask = useAsk(np.meetingId, () => void actions.refetch());

  useEffect(() => {
    if (!meeting || tabFor.current === meeting.id) return;
    tabFor.current = meeting.id;
    setTab(meeting.status === "recording" ? "transcript" : "summary");
  }, [meeting]);

  useEffect(() => {
    if (np.stopped) setTab("summary");
  }, [np.stopped]);

  const [toast, setToast] = useState(false);
  const [freshPromo, setFreshPromo] = useState(false);
  useEffect(() => {
    if (!np.ready) return;
    setToast(true);
    setFreshPromo(false);
    const t = window.setTimeout(() => {
      setToast(false);
      setFreshPromo(true);
    }, TOAST_MS);
    return () => window.clearTimeout(t);
  }, [np.ready]);

  useEffect(() => {
    if (tab !== "summary") setFreshPromo(false);
  }, [tab]);

  useEffect(() => {
    if (!notice) return;
    const t = window.setTimeout(() => setNotice(null), NOTICE_MS);
    return () => window.clearTimeout(t);
  }, [notice]);

  useEffect(() => {
    if (!help) return;
    const t = window.setTimeout(() => setHelp(null), HELP_MS);
    return () => window.clearTimeout(t);
  }, [help]);

  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    atBottom.current = true;
    el.scrollTop = 0;
  }, [tab, np.meetingId]);

  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (el && live && tab === "transcript" && atBottom.current)
      el.scrollTop = el.scrollHeight;
  }, [
    live,
    tab,
    reveal.current,
    reveal.words,
    detail?.segments.length,
    np.partials,
  ]);

  const names = useMemo(() => (detail ? peopleNames(detail) : []), [detail]);

  const copy = async (text: string, done: string) =>
    setNotice((await copyText(text)) ? done : "Couldn't copy");

  const connectCalendar = async () => setHelp(await actions.connectCalendar());

  const onCite = (c: Citation) => {
    if (c.meeting_id !== meeting?.id) return;
    ask.setOpen(false);
    if (c.start_ms > 0) {
      setTab("transcript");
      setFocusMs({ ms: c.start_ms, at: Date.now() });
    } else {
      setTab("summary");
    }
  };

  if (!detail || !meeting)
    return (
      <div className="np">
        <div className="np-bar" data-tauri-drag-region />
        <div className="np-loading">
          {np.meetingId && !np.loaded
            ? ""
            : "No note open. Start a recording from the pill or the Meetings page."}
        </div>
      </div>
    );

  const summaryReady = !!detail.summary && meeting.summary_status === "ready";
  const showCalendar =
    np.calendar !== "full_access" && np.calendar !== "unsupported";

  return (
    <div className="np">
      <TopBar
        showCalendar={showCalendar}
        chatOpen={ask.open && !!ask.history}
        onConnectCalendar={connectCalendar}
        onShare={() =>
          copy(noteMarkdown(detail, false), "Summary and transcript copied")
        }
        onCopyMarkdown={() =>
          copy(noteMarkdown(detail, true), "Note copied as Markdown")
        }
        onCopyLink={() =>
          copy(
            `lipik://meeting/${meeting.id}`,
            "Link copied. Paste it into an AI app connected to Lipik.",
          )
        }
        onCopySummary={() =>
          detail.summary
            ? copy(summaryMarkdown(detail.summary), "Summary copied")
            : setNotice("No summary yet")
        }
        onToggleChats={() =>
          ask.open && ask.history ? ask.setOpen(false) : void ask.showHistory()
        }
        onDelete={actions.remove}
      />
      <Header
        meeting={meeting}
        onRename={actions.rename}
        onFolder={actions.moveToFolder}
      />
      <Tabs tab={tab} onTab={setTab} live={live} generating={generating} />
      <div className="np-body">
        <div
          className="np-scroll"
          ref={scrollRef}
          onScroll={(e) => {
            const el = e.currentTarget;
            atBottom.current =
              el.scrollHeight - el.scrollTop - el.clientHeight < 48;
          }}
        >
          <div className="np-pane" key={tab}>
            {tab === "transcript" && (
              <TranscriptTab
                segments={detail.segments}
                live={live}
                startMs={meeting.start_ms}
                lengthMs={meetingLength(detail)}
                reveal={reveal}
                partials={np.partials}
                callApp={live ? meeting.app : null}
                focusMs={focusMs}
                onCopy={() =>
                  copy(transcriptText(detail.segments), "Transcript copied")
                }
                onSoon={setNotice}
                busy={generating}
                onRenameSpeaker={async (key, name) => {
                  const error = await actions.renameSpeaker(key, name);
                  if (error) setNotice(error);
                }}
              />
            )}
            {tab === "summary" && (
              <SummaryTab
                detail={detail}
                step={step}
                names={names}
                onCopy={() =>
                  detail.summary &&
                  copy(summaryMarkdown(detail.summary), "Summary copied")
                }
                onRegenerate={actions.regenerate}
              />
            )}
            {tab === "thoughts" && (
              <ThoughtsTab
                key={meeting.id}
                meetingId={meeting.id}
                thoughts={detail.thoughts}
                summaryReady={summaryReady}
                onSave={actions.saveThoughts}
                onOpenSummary={() => setTab("summary")}
              />
            )}
          </div>
        </div>
        <div className="np-notices">
          {tab === "transcript" && live && (
            <LiveNotices
              heard={
                detail.segments.length > 0 ||
                Object.values(np.partials).some((l) => !!l?.text)
              }
              streams={np.streams}
            />
          )}
          {tab === "transcript" && live && <div style={{ height: 12 }} />}
          {tab === "summary" && summaryReady && (
            <SummaryNotices
              toast={toast}
              fresh={freshPromo}
              showPromo={showCalendar}
              onConnect={connectCalendar}
            />
          )}
          {tab === "thoughts" && <ThoughtsNote />}
        </div>
      </div>
      <AskDrawer
        ask={ask}
        onCite={onCite}
        onCopy={(t) => copy(t, "Answer copied")}
      />
      <BottomBar
        live={live}
        ask={ask}
        onStop={actions.stop}
        onResume={actions.resume}
        onError={setNotice}
      />
      {notice && (
        <div className="np-copied" role="status">
          {notice}
        </div>
      )}
      {help && !notice && (
        <div className="np-copied wide" role="status">
          {help}
        </div>
      )}
    </div>
  );
};
