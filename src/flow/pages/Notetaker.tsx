import { useRef, useState } from "react";
import { Settings2 } from "lucide-react";
import type { Citation } from "@/bindings";
import { isFlowPreview } from "../dev/preview";
import { useFlowStore } from "../state";
import { Button, IconButton, PageHeader } from "../ui";
import "./notetaker/notetaker.css";
import { AskBox } from "./notetaker/AskBox";
import { AskView, type AskRequest } from "./notetaker/AskView";
import {
  startNotetaker,
  startOfDay,
  stopNotetaker,
  useNotetakerData,
} from "./notetaker/data";
import { MeetingsCard } from "./notetaker/MeetingsCard";
import { NotesSection, type NotesTab } from "./notetaker/NotesSection";
import { NoteView, type NoteTab } from "./notetaker/NoteView";
import { PREVIEW } from "./notetaker/sample";
import {
  ChatsPanel,
  EmptyPanel,
  MeetingPanel,
  NotePanel,
} from "./notetaker/SidePanel";

type Selection = { kind: "meeting" | "note"; id: string };

type View =
  | { kind: "hub" }
  | { kind: "note"; id: string; tab: NoteTab | null; focusMs: number | null }
  | {
      kind: "chat";
      threadId: string | null;
      meetingId: string | null;
      meetingTitle?: string;
    };

const preview = isFlowPreview ? PREVIEW : null;

const NOTE_TABS: NoteTab[] = ["thoughts", "transcript", "summary"];

const initialSelection = (): Selection | null => {
  const id = preview?.select;
  if (!id) return null;
  return { kind: id.startsWith("n") ? "note" : "meeting", id };
};

const initialView = (): View => {
  if (preview?.open) {
    const tab = NOTE_TABS.find((t) => t === preview.noteTab) ?? null;
    return { kind: "note", id: preview.open, tab, focusMs: null };
  }
  if (preview?.ask) return { kind: "chat", threadId: null, meetingId: null };
  return { kind: "hub" };
};

export default function Notetaker() {
  const openSettings = useFlowStore((s) => s.openSettings);
  const openOverlay = useFlowStore((s) => s.openOverlay);
  const {
    meetings,
    calendarAccess,
    notes,
    folders,
    recordingId,
    refreshCalendar,
    reloadLibrary,
  } = useNotetakerData();
  const [tab, setTab] = useState<NotesTab>(
    preview?.tab === "folders" ? "folders" : "past",
  );
  const [picked, setPicked] = useState<Selection | null>(initialSelection);
  const [view, setView] = useState<View>(initialView);
  const [history, setHistory] = useState<View[]>([]);
  const asks = useRef(1);
  const [request, setRequest] = useState<AskRequest | null>(
    preview?.ask ? { question: preview.ask, n: 1 } : null,
  );
  const [chatsVersion, setChatsVersion] = useState(0);

  const go = (next: View) => {
    setHistory((h) => [...h, view]);
    setView(next);
  };
  const back = () => {
    setView(history[history.length - 1] ?? { kind: "hub" });
    setHistory((h) => h.slice(0, -1));
  };

  const openNote = (id: string, tab: NoteTab | null = null) =>
    go({ kind: "note", id, tab, focusMs: null });

  const openCitation = (c: Citation) =>
    go({
      kind: "note",
      id: c.meeting_id,
      tab: c.start_ms > 0 ? "transcript" : "summary",
      focusMs: c.start_ms > 0 ? c.start_ms : null,
    });

  const ask = (question: string, chat: View) => {
    setRequest({ question, n: ++asks.current });
    if (view.kind !== "chat") go(chat);
  };

  const toggleRecording = async () => {
    if (recordingId) {
      await stopNotetaker();
      return;
    }
    const meeting = await startNotetaker(null);
    if (meeting) openNote(meeting.id, "transcript");
  };

  const pickedMeeting =
    picked?.kind === "meeting"
      ? meetings.data.find((m) => m.id === picked.id)
      : undefined;
  const pickedNote =
    picked?.kind === "note"
      ? notes.data.find((n) => n.id === picked.id)
      : undefined;
  const meeting = pickedMeeting ?? (pickedNote ? undefined : meetings.data[0]);
  const note =
    pickedNote ??
    (meeting || picked || tab !== "past" ? undefined : notes.data[0]);
  const today = startOfDay(Date.now());
  const finishedToday = notes.data.filter(
    (n) => n.endedAt !== null && n.endedAt >= today,
  ).length;
  const openedNote =
    view.kind === "note" ? notes.data.find((n) => n.id === view.id) : undefined;

  let head = null;
  let main;
  let panel;
  let askBox;
  if (view.kind === "note") {
    main = (
      <NoteView
        key={view.id}
        id={view.id}
        folders={folders.data}
        initialTab={view.tab}
        focusMs={view.focusMs}
        onBack={back}
        onChanged={reloadLibrary}
        onDeleted={back}
      />
    );
    panel = openedNote ? (
      <NotePanel key={openedNote.id} note={openedNote} showOverview={false} />
    ) : (
      <EmptyPanel />
    );
    askBox = (
      <AskBox
        key="note"
        placeholder="Ask about this meeting"
        onAsk={(q) =>
          ask(q, {
            kind: "chat",
            threadId: null,
            meetingId: view.id,
            meetingTitle: openedNote?.title,
          })
        }
      />
    );
  } else if (view.kind === "chat") {
    main = (
      <AskView
        threadId={view.threadId}
        meetingId={view.meetingId}
        meetingTitle={view.meetingTitle}
        request={request}
        onRequestTaken={() => setRequest(null)}
        onThread={(threadId) => {
          setView({ ...view, threadId });
          setChatsVersion((v) => v + 1);
        }}
        onBack={back}
        onOpenCitation={openCitation}
      />
    );
    panel = (
      <ChatsPanel
        threadId={view.threadId}
        version={chatsVersion}
        onPick={(threadId) =>
          setView({ kind: "chat", threadId, meetingId: null })
        }
        onNew={() => setView({ kind: "chat", threadId: null, meetingId: null })}
      />
    );
    askBox = (
      <AskBox
        key="chat"
        placeholder={
          view.threadId ? "Ask a follow-up" : "Ask about your meetings"
        }
        onAsk={(q) => ask(q, view)}
      />
    );
  } else {
    head = (
      <PageHeader
        title="Meetings"
        description="Notes from your calls, written as you talk."
        actions={
          <>
            <IconButton
              label="Meeting settings"
              onClick={() => openSettings("notetaker")}
            >
              <Settings2 size={18} strokeWidth={1.75} />
            </IconButton>
            {recordingId ? (
              <Button
                key="stop"
                variant="danger"
                icon={
                  <span className="h-[10px] w-[10px] rounded-[2px] bg-current" />
                }
                onClick={() => void toggleRecording()}
              >
                Stop recording
              </Button>
            ) : (
              <Button
                key="start"
                icon={
                  <span className="h-[8px] w-[8px] rounded-full bg-[var(--fk-live)] shadow-[0_0_0_2px_var(--fk-accent-text)]" />
                }
                onClick={() => void toggleRecording()}
              >
                Start recording
              </Button>
            )}
          </>
        }
      />
    );
    main = (
      <>
        <div>
          <MeetingsCard
            meetings={meetings.data}
            loading={meetings.loading}
            access={calendarAccess}
            showToday={!preview?.hideToday}
            finishedToday={finishedToday}
            selectedId={meeting?.id ?? null}
            onSelect={(id) => setPicked({ kind: "meeting", id })}
            onRefresh={refreshCalendar}
            onConnect={() => openOverlay("connect-calendar")}
          />
        </div>
        <NotesSection
          tab={tab}
          onTab={setTab}
          notes={notes}
          folders={folders}
          selectedId={note?.id ?? null}
          pickedId={picked?.kind === "note" ? picked.id : null}
          onSelect={(id) => setPicked({ kind: "note", id })}
          onOpen={(id) => openNote(id)}
          onChanged={reloadLibrary}
        />
      </>
    );
    panel = meeting ? (
      <MeetingPanel key={meeting.id} meeting={meeting} />
    ) : note ? (
      <NotePanel key={note.id} note={note} />
    ) : (
      <EmptyPanel />
    );
    askBox = (
      <AskBox
        key="hub"
        onAsk={(q) => ask(q, { kind: "chat", threadId: null, meetingId: null })}
        onPastChats={() =>
          go({ kind: "chat", threadId: null, meetingId: null })
        }
      />
    );
  }

  return (
    <div className="nt relative h-full">
      <div key={view.kind} className="fk-scroll absolute inset-0">
        <div className="fk-page pb-[112px]">
          {head}
          <div className="grid grid-cols-[minmax(0,1fr)_280px] gap-x-[32px]">
            <div className="nt-fade min-w-0">{main}</div>
            <aside className="fk-scroll sticky top-[8px] max-h-[calc(100vh-200px)] self-start rounded-[14px] bg-[var(--fk-surface)] p-[20px]">
              {panel}
            </aside>
          </div>
        </div>
      </div>
      <div className="pointer-events-none absolute inset-x-0 bottom-0 h-[96px] bg-[linear-gradient(to_bottom,transparent,var(--fk-panel)_32px)]" />
      <div className="absolute inset-x-0 bottom-[24px]">
        <div className="mx-auto grid max-w-[1080px] grid-cols-[minmax(0,1fr)_280px] gap-x-[32px] px-[48px]">
          {askBox}
        </div>
      </div>
    </div>
  );
}
