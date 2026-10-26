import { Bell } from "lucide-react";
import { useEffect, useRef } from "react";
import { MENU_TOP, useEscape } from "./Popover";
import type { OverlayProps } from "./types";

const TRIGGER = 'header button[aria-label="Notifications"]';

export default function Notifications({ onClose }: OverlayProps) {
  const ref = useRef<HTMLDivElement>(null);
  useEscape(onClose);
  useEffect(() => ref.current?.focus(), []);
  useEffect(() => {
    const onDown = (e: MouseEvent) => {
      const target = e.target as Element;
      if (ref.current?.contains(target) || target.closest(TRIGGER)) return;
      onClose();
    };
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [onClose]);

  return (
    <div
      ref={ref}
      role="dialog"
      aria-label="Notifications"
      tabIndex={-1}
      className="fk-menu fixed z-40 overflow-hidden rounded-[var(--fk-r-lg)] bg-[var(--fk-menu-bg)] shadow-[var(--fk-elev-2)] outline-none"
      style={{ top: MENU_TOP, right: 46, width: 340 }}
    >
      <div className="flex h-[48px] items-center border-b border-[var(--fk-border)] px-[16px] text-[15px] font-semibold">
        Notifications
      </div>
      <div className="flex items-start gap-[12px] px-[16px] py-[20px]">
        <Bell
          size={18}
          strokeWidth={1.75}
          className="mt-[2px] shrink-0 text-[var(--fk-text-faint)]"
        />
        <div>
          <div className="text-[14.5px] font-medium leading-[21px]">
            No notifications yet
          </div>
          <div className="mt-[2px] text-[13px] leading-[18px] text-[var(--fk-text-muted)]">
            Meeting notes and updates appear here.
          </div>
        </div>
      </div>
    </div>
  );
}
