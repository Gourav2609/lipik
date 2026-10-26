import { CircleUser, Info, Settings2, type LucideIcon } from "lucide-react";
import { useFlowStore, type SettingsTab } from "../state";
import { useProfile } from "../settings/profile";
import { MENU_TOP, Popover, TOP_BAR_TRIGGERS } from "./Popover";
import type { OverlayProps } from "./types";
import { APP_NAME } from "@/flow/brand";

const ITEMS: { label: string; tab: SettingsTab; Icon: LucideIcon }[] = [
  { label: "Profile", tab: "account", Icon: CircleUser },
  { label: "Settings", tab: "general", Icon: Settings2 },
  { label: `About ${APP_NAME}`, tab: "billing", Icon: Info },
];

export default function AccountMenu({ onClose }: OverlayProps) {
  const openSettings = useFlowStore((s) => s.openSettings);
  const { profile } = useProfile();
  const name = `${profile.firstName} ${profile.lastName}`.trim();
  const initial = (name || profile.email || "?").charAt(0).toUpperCase();

  return (
    <Popover
      onClose={onClose}
      ignore={TOP_BAR_TRIGGERS}
      style={{ top: MENU_TOP, right: 12, width: 280 }}
      className="fk-menu"
    >
      <div className="flex items-center gap-[12px] border-b border-[var(--fk-border)] px-[16px] py-[16px]">
        {profile.picture ? (
          <img
            src={profile.picture}
            alt=""
            className="h-[40px] w-[40px] shrink-0 rounded-full object-cover"
          />
        ) : (
          <span className="fk-display flex h-[40px] w-[40px] shrink-0 items-center justify-center rounded-full bg-[var(--fk-accent-soft)] text-[18px] text-[var(--fk-accent)]">
            {initial}
          </span>
        )}
        <div className="min-w-0">
          <div className="truncate text-[15px] font-semibold leading-[22px]">
            {name || "No name yet"}
          </div>
          <div className="truncate text-[13px] leading-[18px] text-[var(--fk-text-muted)]">
            {profile.email || "Your profile stays on this Mac"}
          </div>
        </div>
      </div>
      <div className="p-[4px]">
        {ITEMS.map(({ label, tab, Icon }) => (
          <button
            key={tab}
            type="button"
            role="menuitem"
            onClick={() => openSettings(tab)}
            className="flex h-[34px] w-full items-center gap-[10px] rounded-[var(--fk-r-sm)] px-[12px] text-[14.5px] transition-colors duration-[var(--fk-t-quick)] hover:bg-[var(--fk-hover)] focus-visible:bg-[var(--fk-hover)]"
          >
            <Icon
              size={16}
              strokeWidth={1.75}
              className="text-[var(--fk-text-muted)]"
            />
            {label}
          </button>
        ))}
      </div>
    </Popover>
  );
}
