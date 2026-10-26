import { useEffect } from "react";
import type { OverlayProps } from "./types";
import "./overlays.css";

const VISIBLE_MS = 3200;

export default function Toast({ onClose }: OverlayProps) {
  useEffect(() => {
    const id = window.setTimeout(onClose, VISIBLE_MS);
    return () => window.clearTimeout(id);
  }, [onClose]);

  return (
    <div
      role="status"
      onClick={onClose}
      className="fk-toast fixed bottom-[16px] right-[16px] z-50 flex h-[36px] items-center rounded-[var(--fk-r-md)] bg-[var(--fk-inverse-bg)] px-[14px] text-[14px] font-medium text-[var(--fk-inverse-text)] shadow-[var(--fk-elev-2)]"
    >
      Last dictation copied
    </div>
  );
}
