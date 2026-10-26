import { PageHeader } from "../ui";
import { ActivityGrid } from "./insights/ActivityGrid";
import { AppUsageList } from "./insights/AppUsage";
import { CopyStats } from "./insights/CopyStats";
import { StatStrip } from "./insights/StatStrip";
import { useInsightsStats } from "./insights/useInsightsStats";
import "./insights/insights.css";
import { APP_NAME } from "@/flow/brand";

const shareText = (words: number, streak: number) =>
  `${words.toLocaleString("en-US")} words written by voice with ${APP_NAME}` +
  (streak > 1 ? `, ${streak} days in a row.` : ".");

export default function Insights() {
  const { loaded, stats, wpm, wordsCorrected, dictionaryFixes, usage } =
    useInsightsStats();

  return (
    <div className="fk-page fk-insights">
      <PageHeader
        title="Stats"
        description="How much you write, and how fast."
        actions={
          <CopyStats text={shareText(stats.totalWords, stats.currentStreak)} />
        }
      />
      <StatStrip
        stats={stats}
        wpm={wpm}
        wordsCorrected={wordsCorrected}
        dictionaryFixes={dictionaryFixes}
      />
      <div className="mt-[40px] grid grid-cols-[minmax(0,1fr)_minmax(0,1fr)] gap-[48px]">
        <ActivityGrid stats={stats} loaded={loaded} />
        <AppUsageList usage={usage} loaded={loaded} />
      </div>
    </div>
  );
}
