import type { ReactNode } from "react";
import { Ellipsis, Info } from "lucide-react";
import { isFlowPreview } from "../dev/preview";
import { Button, IconButton } from "../ui";
import { SettingsCard, useLocalSetting } from "./parts";

/** Neutral tile for an app: a Lucide icon, or the app's initial when none is given. */
export const AppTile = ({ name, icon }: { name: string; icon?: ReactNode }) => (
  <span className="flex h-[32px] w-[32px] shrink-0 items-center justify-center rounded-[var(--fk-r-sm)] bg-[var(--fk-menu-bg)] text-[var(--fk-text-muted)] shadow-[var(--fk-elev-1)]">
    {icon ?? (
      <span className="fk-display text-[15px] leading-none text-[var(--fk-text)]">
        {name.charAt(0)}
      </span>
    )}
  </span>
);

export interface AppEntry {
  id: string;
  name: string;
  icon?: ReactNode;
  description: string;
  action: string;
  /** Tooltip on an info icon left of the button. */
  note?: string;
}

/** Ruled list of apps with a connect button, or "…" once connected.
 *  Nothing can be connected yet, so outside the preview the buttons only
 *  say so; `initial` is preview sample state. */
export const AppList = ({
  storageKey,
  apps,
  initial = [],
}: {
  storageKey: string;
  apps: AppEntry[];
  initial?: string[];
}) => {
  const [connected, setConnected] = useLocalSetting<string[]>(
    storageKey,
    initial,
  );
  const shown = isFlowPreview ? connected : [];
  const pending = isFlowPreview ? undefined : "Not available yet";
  const toggle = (id: string) =>
    isFlowPreview &&
    setConnected(
      connected.includes(id)
        ? connected.filter((c) => c !== id)
        : [...connected, id],
    );

  return (
    <SettingsCard>
      {apps.map(({ id, name, icon, description, action, note }) => (
        <div
          key={id}
          className="flex min-h-[72px] items-center gap-[12px] border-b border-[var(--fk-border)] py-[16px] last:border-b-0"
        >
          <AppTile name={name} icon={icon} />
          <div className="min-w-0 flex-1">
            <div className="text-[15px] font-medium leading-[22px]">{name}</div>
            <div className="mt-[2px] text-[13px] leading-[18px] text-[var(--fk-text-muted)]">
              {description}
            </div>
          </div>
          <div className="flex shrink-0 items-center gap-[8px]">
            {note && (
              <span
                aria-label={note}
                title={note}
                className="flex text-[var(--fk-text-faint)]"
              >
                <Info size={16} strokeWidth={1.75} />
              </span>
            )}
            {shown.includes(id) ? (
              <>
                <span className="text-[13px] font-medium text-[var(--fk-ok)]">
                  Connected
                </span>
                <IconButton
                  label={`Disconnect ${name}`}
                  onClick={() => toggle(id)}
                >
                  <Ellipsis size={16} strokeWidth={1.75} />
                </IconButton>
              </>
            ) : (
              <Button
                variant="secondary"
                size="sm"
                title={pending}
                onClick={() => toggle(id)}
              >
                {action}
              </Button>
            )}
          </div>
        </div>
      ))}
    </SettingsCard>
  );
};
