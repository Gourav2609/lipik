import { useEffect, useRef, useState } from "react";
import { useOsType } from "@/hooks/useOsType";
import { useSettings } from "@/hooks/useSettings";
import { formatKeyCombination } from "@/lib/utils/keyboard";
import { APP_NAME } from "../brand";
import { isFlowPreview } from "../dev/preview";
import { useProfile } from "../settings/profile";
import { useFlowStore } from "../state";
import { HeroBanner, HeroButton, Kbd, PageHeader } from "../ui";
import { HistoryList } from "./dictation/HistoryList";
import { HomeStats } from "./dictation/HomeStats";
import { useHistory } from "./dictation/useHistory";
import { useInsightsStats } from "./insights/useInsightsStats";

const params = new URLSearchParams(window.location.search);
const previewScroll = isFlowPreview ? Number(params.get("scroll") ?? 0) : 0;
const previewHover =
  isFlowPreview && params.has("hover") ? Number(params.get("hover")) : undefined;
const previewEdit =
  isFlowPreview && params.has("edit") ? Number(params.get("edit")) : undefined;
const previewMenu =
  isFlowPreview && params.has("menu") ? Number(params.get("menu")) : undefined;
const previewQuery = isFlowPreview ? (params.get("q") ?? undefined) : undefined;

const NOTE_KEY = "lipik.home.vocabularyNoteDismissed";

const readDismissed = () => {
  if (isFlowPreview) return params.has("noteHidden");
  try {
    return localStorage.getItem(NOTE_KEY) === "1";
  } catch {
    return false;
  }
};

const greeting = () => {
  const h = new Date().getHours();
  if (h < 5 || h >= 17) return "Good evening";
  if (h < 12) return "Good morning";
  return "Good afternoon";
};

const ShortcutKeys = ({ binding }: { binding: string }) => (
  <span className="mx-[2px] inline-flex gap-[4px] align-baseline">
    {formatKeyCombination(binding, useOsType())
      .split(" + ")
      .map((k) => (
        <Kbd key={k}>{k}</Kbd>
      ))}
  </span>
);

export default function Dictation() {
  const setPage = useFlowStore((s) => s.setPage);
  const { entries, loading, toggleSaved, remove, retry, updateText } = useHistory();
  const { stats, wpm } = useInsightsStats();
  const rootRef = useRef<HTMLDivElement>(null);
  const [noteHidden, setNoteHidden] = useState(readDismissed);
  const name = useProfile().profile.firstName.trim().split(/\s+/)[0];
  const binding =
    useSettings().settings?.bindings?.transcribe?.current_binding || "fn";

  useEffect(() => {
    if (!loading && previewScroll) {
      rootRef.current?.closest(".fk-scroll")?.scrollTo(0, previewScroll);
    }
  }, [loading]);

  const hideNote = () => {
    setNoteHidden(true);
    try {
      localStorage.setItem(NOTE_KEY, "1");
    } catch {
      // Storage can be unavailable; the note then shows again next launch.
    }
  };

  return (
    <div ref={rootRef} className="fk-page">
      <PageHeader
        className="mt-[24px]"
        title={name ? `${greeting()}, ${name}` : greeting()}
        description={
          <>
            Hold <ShortcutKeys binding={binding} /> to write anywhere.
          </>
        }
        actions={
          <HomeStats
            totalWords={stats.totalWords}
            wpm={wpm}
            streak={stats.currentStreak}
            onOpen={() => setPage("insights")}
          />
        }
      />
      {!noteHidden && (
        <HeroBanner
          className="mb-[40px]"
          title={`Teach ${APP_NAME} your words`}
          subtitle="Add the names, places and terms you use, so they come out right the first time."
          actions={
            <HeroButton strong onClick={() => setPage("dictionary")}>
              Add words
            </HeroButton>
          }
          onClose={hideNote}
        />
      )}
      <HistoryList
        entries={entries}
        loading={loading}
        onToggleSaved={toggleSaved}
        onRetry={retry}
        onDelete={remove}
        onUpdateText={updateText}
        hoverIndex={previewHover}
        editIndex={previewEdit}
        menuIndex={previewMenu}
        initialQuery={previewQuery}
      />
    </div>
  );
}
