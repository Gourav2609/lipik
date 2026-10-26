import type { ComponentType } from "react";
import {
  BookA,
  ChartNoAxesColumn,
  House,
  LifeBuoy,
  NotebookPen,
  NotepadText,
  PencilLine,
  Settings2,
  Signature,
  TextQuote,
  type LucideProps,
} from "lucide-react";
import { useOsType } from "@/hooks/useOsType";
import { useSettings } from "@/hooks/useSettings";
import { formatKeyCombination } from "@/lib/utils/keyboard";
import { useFlowStore, type PageId } from "./state";
import { APP_NAME } from "./brand";
import { Kbd, LipikMark } from "./ui";

type IconComponent = ComponentType<LucideProps>;

export const NAV_ITEMS: {
  id: PageId;
  label: string;
  Icon: IconComponent;
  iconSize?: number;
}[] = [
  { id: "dictation", label: "Home", Icon: House },
  { id: "notetaker", label: "Meetings", Icon: NotebookPen },
  { id: "scratchpad", label: "Drafts", Icon: NotepadText },
  { id: "dictionary", label: "Vocabulary", Icon: BookA },
  { id: "snippets", label: "Snippets", Icon: TextQuote },
  { id: "style", label: "Tone", Icon: Signature },
  { id: "transforms", label: "Rewrite", Icon: PencilLine },
  { id: "insights", label: "Stats", Icon: ChartNoAxesColumn },
];

const NavItem = ({
  Icon,
  iconSize = 18,
  label,
  active = false,
  rail,
  onClick,
}: {
  Icon: IconComponent;
  iconSize?: number;
  label: string;
  active?: boolean;
  rail: boolean;
  onClick?: () => void;
}) => (
  <button
    type="button"
    onClick={onClick}
    aria-label={label}
    aria-current={active ? "page" : undefined}
    data-rail={rail || undefined}
    className={`fk-nav-item relative flex w-full shrink-0 items-center overflow-hidden ${rail ? "h-[52px] flex-col justify-center gap-[3px] rounded-[10px]" : "h-[34px] gap-[10px] rounded-[8px] pl-[12px]"} ${active ? "bg-[var(--fk-menu-bg)] text-[var(--fk-text)] shadow-[var(--fk-elev-1)]" : "text-[var(--fk-text-muted)] hover:bg-[var(--fk-hover)] hover:text-[var(--fk-text)]"}`}
  >
    <Icon
      size={iconSize}
      strokeWidth={1.75}
      className={`shrink-0 ${active ? "text-[var(--fk-accent)]" : ""}`}
    />
    <span
      className={`whitespace-nowrap ${rail ? "text-[10.5px] font-medium leading-[12px]" : "text-[14.5px] font-medium leading-none"}`}
    >
      {label}
    </span>
  </button>
);

const HoldHint = () => {
  const binding =
    useSettings().settings?.bindings?.transcribe?.current_binding || "fn";
  const keys = formatKeyCombination(binding, useOsType()).split(" + ");
  return (
    <div className="mb-[12px] shrink-0 rounded-[10px] bg-[var(--fk-surface-2)] px-[12px] py-[12px]">
      <div className="flex flex-wrap items-center gap-[5px] text-[13.5px] font-semibold leading-[22px]">
        Hold
        {keys.map((k) => (
          <Kbd key={k}>{k}</Kbd>
        ))}
        and speak
      </div>
      <p className="mt-[2px] text-[12.5px] leading-[17px] text-[var(--fk-text-muted)]">
        Text lands where your cursor is.
      </p>
    </div>
  );
};

export const Sidebar = ({ fullscreen = false }: { fullscreen?: boolean }) => {
  const page = useFlowStore((s) => s.page);
  const rail = useFlowStore((s) => s.sidebarCollapsed);
  const setPage = useFlowStore((s) => s.setPage);
  const openSettings = useFlowStore((s) => s.openSettings);
  const openOverlay = useFlowStore((s) => s.openOverlay);

  const footer: { label: string; Icon: IconComponent; onClick?: () => void }[] =
    [
      { label: "Settings", Icon: Settings2, onClick: () => openSettings() },
      { label: "Help", Icon: LifeBuoy },
    ];

  return (
    <aside className="fk-sidebar absolute inset-y-0 left-0 flex flex-col overflow-hidden bg-[var(--fk-bg)]">
      <div
        data-tauri-drag-region
        className={`flex h-[var(--fk-header-h)] shrink-0 items-center ${rail ? "" : fullscreen ? "pl-[20px]" : "pl-[96px]"}`}
      >
        {!rail && <Brand onClick={() => openOverlay("confetti")} />}
      </div>
      <div
        key={rail ? "rail" : "full"}
        className={`fk-redraw flex min-h-0 flex-1 flex-col ${rail ? "px-[8px]" : "px-[12px]"}`}
      >
        {rail && (
          <button
            type="button"
            aria-label={APP_NAME}
            onClick={() => openOverlay("confetti")}
            className={`mx-auto mb-[12px] flex shrink-0 text-[var(--fk-accent)] ${fullscreen ? "" : "mt-[4px]"}`}
          >
            <LipikMark size={22} />
          </button>
        )}
        <nav
          className={`flex flex-col ${rail ? "gap-[4px]" : "mt-[8px] gap-[2px]"}`}
        >
          {NAV_ITEMS.map(({ id, label, Icon, iconSize }) => (
            <NavItem
              key={id}
              Icon={Icon}
              iconSize={iconSize}
              label={label}
              rail={rail}
              active={page === id}
              onClick={() => setPage(id)}
            />
          ))}
        </nav>
        <div className="flex-1" />
        {!rail && <HoldHint />}
        <div
          className={`flex flex-col border-t border-[var(--fk-border)] pb-[12px] pt-[8px] ${rail ? "gap-[4px]" : "gap-[2px]"}`}
        >
          {footer.map(({ label, Icon, onClick }) => (
            <NavItem
              key={label}
              Icon={Icon}
              label={label}
              rail={rail}
              onClick={onClick}
            />
          ))}
        </div>
      </div>
    </aside>
  );
};

const Brand = ({ onClick }: { onClick: () => void }) => (
  <button
    type="button"
    onClick={onClick}
    aria-label={APP_NAME}
    className="flex items-center gap-[7px] text-[var(--fk-text)]"
  >
    <LipikMark size={20} className="shrink-0 text-[var(--fk-accent)]" />
    <span className="fk-display text-[18px] font-semibold leading-none">
      {APP_NAME}
    </span>
  </button>
);
