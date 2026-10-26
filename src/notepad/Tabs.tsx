import type { ReactNode } from "react";
import { LiveBars } from "./icons";

export type Tab = "thoughts" | "transcript" | "summary";

type Props = {
  tab: Tab;
  onTab: (tab: Tab) => void;
  live: boolean;
  generating: boolean;
};

export const Tabs = ({ tab, onTab, live, generating }: Props) => {
  const button = (id: Tab, label: string, icon?: ReactNode) => (
    <button
      key={id}
      type="button"
      role="tab"
      aria-selected={tab === id}
      className={`np-tab${tab === id ? " on" : ""}`}
      onClick={() => onTab(id)}
    >
      {icon}
      {label}
    </button>
  );

  return (
    <div className="np-tabs">
      <div className="np-tabs-track" role="tablist">
        {button(
          "summary",
          "Summary",
          generating ? <span className="np-dot" aria-hidden /> : undefined,
        )}
        {button("transcript", "Transcript", live ? <LiveBars /> : undefined)}
        {button("thoughts", "My notes")}
      </div>
    </div>
  );
};
