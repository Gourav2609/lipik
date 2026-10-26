import { Check, Monitor, Moon, Sun } from "lucide-react";
import type { Theme } from "@/bindings";
import { useSettings } from "@/hooks/useSettings";
import { applyTheme } from "@/lib/utils/theme";
import { MENU_TOP, Popover, TOP_BAR_TRIGGERS } from "./Popover";
import type { OverlayProps } from "./types";

const OPTIONS: { id: Theme; label: string; Icon: typeof Sun }[] = [
  { id: "light", label: "Light", Icon: Sun },
  { id: "dark", label: "Dark", Icon: Moon },
  { id: "system", label: "Match system", Icon: Monitor },
];

export default function ThemeMenu({ onClose }: OverlayProps) {
  const { settings, updateSetting } = useSettings();
  const current = settings?.theme ?? "system";
  return (
    <Popover
      onClose={onClose}
      ignore={TOP_BAR_TRIGGERS}
      style={{ top: MENU_TOP, right: 80, width: 184 }}
      className="fk-menu p-[4px]"
    >
      {OPTIONS.map(({ id, label, Icon }) => {
        const selected = current === id;
        return (
          <button
            key={id}
            type="button"
            role="menuitemradio"
            aria-checked={selected}
            onClick={() => {
              applyTheme(id);
              updateSetting("theme", id);
              onClose();
            }}
            className="flex h-[34px] w-full items-center gap-[10px] rounded-[var(--fk-r-sm)] px-[10px] text-[14.5px] transition-colors duration-[var(--fk-t-quick)] hover:bg-[var(--fk-hover)] focus-visible:bg-[var(--fk-hover)]"
          >
            <Icon
              size={16}
              strokeWidth={1.75}
              className={selected ? "text-[var(--fk-accent)]" : "text-[var(--fk-text-muted)]"}
            />
            <span className="flex-1 text-left">{label}</span>
            {selected && (
              <Check size={16} strokeWidth={1.75} className="text-[var(--fk-accent)]" />
            )}
          </button>
        );
      })}
    </Popover>
  );
}
