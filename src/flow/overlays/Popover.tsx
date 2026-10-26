import {
  useEffect,
  useRef,
  type CSSProperties,
  type KeyboardEvent,
  type ReactNode,
} from "react";

export const useEscape = (onClose: () => void) =>
  useEffect(() => {
    const onKey = (e: globalThis.KeyboardEvent) => e.key === "Escape" && onClose();
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

/** The top-bar buttons that open the appearance and account menus. */
export const TOP_BAR_TRIGGERS = 'header button[aria-label="Appearance"], header button[aria-label="Account"]';

/** Top edge of menus opened from the header row: 52 px header + 4 px. */
export const MENU_TOP = 56;

const ITEMS = '[role^="menuitem"]:not(:disabled)';

const moveFocus = (e: KeyboardEvent<HTMLDivElement>) => {
  const step = e.key === "ArrowDown" ? 1 : e.key === "ArrowUp" ? -1 : 0;
  if (!step) return;
  e.preventDefault();
  const items = [...e.currentTarget.querySelectorAll<HTMLElement>(ITEMS)];
  const at = items.indexOf(document.activeElement as HTMLElement);
  items[at < 0 && step < 0 ? items.length - 1 : (at + step + items.length) % items.length]?.focus();
};

/**
 * Positioned menu box. Closes on Escape or a mousedown outside it. `ignore`
 * selects the buttons that open the menu: they toggle it themselves, so a
 * mousedown on them must not close it first.
 */
export const Popover = ({
  onClose,
  style,
  ignore,
  className = "",
  children,
}: {
  onClose: () => void;
  style: CSSProperties;
  ignore?: string;
  className?: string;
  children: ReactNode;
}) => {
  const ref = useRef<HTMLDivElement>(null);
  useEscape(onClose);
  useEffect(() => ref.current?.focus(), []);
  useEffect(() => {
    const onDown = (e: MouseEvent) => {
      const target = e.target as Element;
      if (ref.current?.contains(target) || (ignore && target.closest(ignore))) return;
      onClose();
    };
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [onClose, ignore]);

  return (
    <div
      ref={ref}
      role="menu"
      tabIndex={-1}
      onKeyDown={moveFocus}
      className={`fixed z-40 overflow-hidden rounded-[var(--fk-r-lg)] bg-[var(--fk-menu-bg)] shadow-[var(--fk-elev-2)] outline-none ${className}`}
      style={style}
    >
      {children}
    </div>
  );
};
