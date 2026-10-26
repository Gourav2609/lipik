import type { LucideIcon } from "lucide-react";
import {
  AppWindow,
  AtSign,
  Cpu,
  MessagesSquare,
  NotebookText,
  Users,
} from "lucide-react";
import { ListGroup } from "../../ui";

export type AppType =
  | "ai"
  | "other"
  | "work"
  | "email"
  | "personal"
  | "document";

const APP_TYPES: { id: AppType; icon: LucideIcon; label: string }[] = [
  { id: "document", icon: NotebookText, label: "Documents" },
  { id: "work", icon: Users, label: "Team chat" },
  { id: "personal", icon: MessagesSquare, label: "Messages" },
  { id: "email", icon: AtSign, label: "Email" },
  { id: "ai", icon: Cpu, label: "AI tools" },
  { id: "other", icon: AppWindow, label: "Other apps" },
];

export const APP_TYPE_IDS = APP_TYPES.map((t) => t.id);

export interface AppUsage {
  /** Dictations per app type. */
  counts: Record<AppType, number>;
  /** Distinct apps dictated into, not app types. */
  appsUsed: number;
}

/** `usage` is null until a dictation has recorded which app it went to. */
export const AppUsageList = ({
  usage,
  loaded,
}: {
  usage: AppUsage | null;
  loaded: boolean;
}) => {
  const count = (t: AppType) => usage?.counts[t] ?? 0;
  const total = APP_TYPES.reduce((sum, t) => sum + count(t.id), 0);
  const top = Math.max(0, ...APP_TYPES.map((t) => count(t.id)));
  const rank = [...APP_TYPES].sort((a, b) => count(b.id) - count(a.id));

  return (
    <section aria-label="Where you write">
      <div className="flex h-[30px] items-center gap-[8px]">
        <h2 className="text-[16px] font-semibold leading-[22px]">
          Where you write
        </h2>
        <span className="fk-num text-[13px] text-[var(--fk-text-faint)]">
          {usage
            ? `${usage.appsUsed} ${usage.appsUsed === 1 ? "app" : "apps"}`
            : "Shows after your next dictation"}
        </span>
      </div>
      <ListGroup className="mt-[16px]">
        {rank.map((u) => {
          const n = count(u.id);
          const pct = total ? Math.round((n / total) * 100) : 0;
          const Icon = u.icon;
          return (
            <div
              key={u.id}
              className="flex h-[44px] items-center gap-[12px] border-b border-[var(--fk-border)] text-[14.5px]"
            >
              <Icon
                size={16}
                strokeWidth={1.75}
                className="shrink-0 text-[var(--fk-text-muted)]"
              />
              <span className="min-w-0 flex-1 truncate">{u.label}</span>
              <span className="h-[4px] w-[112px] shrink-0 overflow-hidden rounded-[2px] bg-[var(--fk-accent-soft)]">
                <span
                  className={`ins-bar block h-full rounded-[2px] bg-[var(--fk-accent)]${usage && loaded ? " ins-in" : ""}`}
                  style={{ width: `${top ? (n / top) * 100 : 0}%` }}
                />
              </span>
              <span className="fk-num w-[44px] shrink-0 text-right">
                {usage ? n.toLocaleString("en-US") : "—"}
              </span>
              <span className="fk-num w-[40px] shrink-0 text-right text-[13px] text-[var(--fk-text-faint)]">
                {usage ? `${pct}%` : ""}
              </span>
            </div>
          );
        })}
      </ListGroup>
    </section>
  );
};
