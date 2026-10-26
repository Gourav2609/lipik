import { useState, type ReactNode } from "react";
import { ChevronDown, Plus, RotateCcw } from "lucide-react";
import {
  Button,
  HeroBanner,
  HeroButton,
  ListGroup,
  ListRow,
  PageHeader,
  Toggle,
} from "../ui";
import { useFlowStore } from "../state";
import { isFlowPreview } from "../dev/preview";
import { OptKeys } from "./transforms/Keycap";
import { HowItWorks } from "./transforms/HowItWorks";
import { TransformDialog } from "./transforms/TransformDialog";
import { confirmAction } from "./transforms/confirm";
import {
  transformsActions as act,
  useTransforms,
  type Transform,
} from "./transforms/store";
import { APP_NAME } from "@/flow/brand";

const CARD =
  "flex min-h-[136px] flex-col items-start rounded-[14px] p-[20px] text-left transition-shadow duration-[var(--fk-t-base)] ease-[var(--fk-ease)]";
const CARD_TITLE =
  "mt-[16px] w-full truncate text-[16px] font-semibold leading-[22px]";
const CARD_TEXT =
  "mt-[4px] w-full text-[14px] leading-[20px] text-[var(--fk-text-muted)]";

const confirmReset = async () => {
  if (
    await confirmAction(
      "Reset rewrites? Your own rewrites will be deleted and the built-in ones restored.",
      "Reset rewrites",
    )
  )
    act.resetTransforms();
};

const SettingText = ({
  title,
  children,
}: {
  title: string;
  children: ReactNode;
}) => (
  <div className="py-[12px]">
    <div className="text-[15px] font-semibold leading-[22px]">{title}</div>
    <div className="mt-[2px] text-[14px] leading-[22px] text-[var(--fk-text-muted)]">
      {children}
    </div>
  </div>
);

type Editing = { mode: "new" } | { mode: "edit"; transform: Transform } | null;

// ?transformsDialog=new|how opens that dialog in the browser preview.
const previewDialog = isFlowPreview
  ? new URLSearchParams(window.location.search).get("transformsDialog")
  : null;

export default function Transforms() {
  const s = useTransforms();
  const setPage = useFlowStore((st) => st.setPage);
  const [editing, setEditing] = useState<Editing>(
    previewDialog === "new" ? { mode: "new" } : null,
  );
  const [howOpen, setHowOpen] = useState(previewDialog === "how");

  return (
    <div className="fk-page">
      <PageHeader
        title={
          <span className="flex items-center gap-[10px]">
            Rewrite
            <span className="rounded-[4px] bg-[var(--fk-inverse-bg)] px-[6px] py-[4px] [font-family:var(--fk-font-sans)] text-[11px] font-semibold leading-none text-[var(--fk-inverse-text)]">
              Beta
            </span>
          </span>
        }
        description="Select text, then press a shortcut to rewrite it."
      />

      <HeroBanner
        title="Rewrite text in any app"
        subtitle={`Select text and press ⌥ with a number. ${APP_NAME} rewrites it in place with that prompt.`}
        actions={
          <>
            <HeroButton strong onClick={() => setPage("scratchpad")}>
              Try in Drafts
            </HeroButton>
            <HeroButton onClick={() => setHowOpen(true)}>
              How it works
            </HeroButton>
          </>
        }
      />

      <ListGroup className="mt-[32px]">
        <ListRow
          showActions
          className="hover:bg-transparent"
          actions={
            <Toggle
              checked={s.optIn}
              onChange={act.setOptIn}
              label="Rewrite shortcuts"
            />
          }
        >
          <SettingText title="Rewrite shortcuts">
            Press <OptKeys keyLabel="1" /> on selected text to rewrite it.{" "}
            <OptKeys keyLabel="o" /> shows what changed.
          </SettingText>
        </ListRow>
        <ListRow
          showActions
          className="hover:bg-transparent"
          actions={
            <div className="flex items-center gap-[12px]">
              <label
                className={`relative flex h-[36px] w-[200px] items-center rounded-[10px] border border-transparent bg-[var(--fk-surface)] transition-colors duration-[var(--fk-t-quick)] focus-within:border-[var(--fk-accent)] focus-within:shadow-[var(--fk-focus)] hover:border-[var(--fk-border)] ${s.autoApply ? "text-[var(--fk-text)]" : "text-[var(--fk-text-muted)]"}`}
              >
                <select
                  aria-label="Rewrite to run on every dictation"
                  value={s.autoApplyId}
                  onChange={(e) => act.setAutoApplyId(e.target.value)}
                  className="h-full w-full appearance-none bg-transparent pl-[12px] pr-[36px] text-[14.5px] outline-none"
                >
                  {s.transforms.map((t) => (
                    <option key={t.id} value={t.id}>
                      {t.name}
                    </option>
                  ))}
                </select>
                <ChevronDown
                  size={16}
                  strokeWidth={1.75}
                  className="pointer-events-none absolute right-[12px] text-[var(--fk-text-muted)]"
                />
              </label>
              <Toggle
                checked={s.autoApply}
                onChange={act.setAutoApply}
                label="Rewrite every dictation"
              />
            </div>
          }
        >
          <SettingText title="Rewrite every dictation">
            Run one rewrite on everything you dictate, before it is typed.
          </SettingText>
        </ListRow>
      </ListGroup>

      <div className="mt-[40px] flex items-center gap-[12px]">
        <h2 className="text-[16px] font-semibold leading-[22px]">
          Your rewrites
          <span className="fk-num ml-[8px] font-normal text-[var(--fk-text-faint)]">
            {s.transforms.length}
          </span>
        </h2>
        <div className="ml-auto flex items-center gap-[8px]">
          <Button
            variant="ghost"
            size="sm"
            icon={<RotateCcw size={16} strokeWidth={1.75} />}
            onClick={confirmReset}
          >
            Reset
          </Button>
          <Button
            size="sm"
            icon={<Plus size={16} strokeWidth={1.75} />}
            onClick={() => setEditing({ mode: "new" })}
          >
            New rewrite
          </Button>
        </div>
      </div>

      <div className="mt-[16px] grid grid-cols-3 gap-[16px]">
        {s.transforms.map((t) => (
          <button
            key={t.id}
            type="button"
            className={`${CARD} bg-[var(--fk-menu-bg)] shadow-[var(--fk-elev-1)] hover:shadow-[var(--fk-elev-2)]`}
            onClick={() => setEditing({ mode: "edit", transform: t })}
          >
            {t.shortcut !== null ? (
              <OptKeys keyLabel={String(t.shortcut)} />
            ) : (
              <span className="h-[22px] text-[13px] leading-[22px] text-[var(--fk-text-faint)]">
                No shortcut
              </span>
            )}
            <span className={CARD_TITLE}>{t.name}</span>
            <span className={`${CARD_TEXT} line-clamp-2`}>{t.description}</span>
          </button>
        ))}
        <button
          type="button"
          className={`${CARD} bg-[var(--fk-surface)] hover:bg-[var(--fk-hover)]`}
          onClick={() => setEditing({ mode: "new" })}
        >
          <span className="flex h-[22px] w-[22px] items-center justify-center rounded-full bg-[var(--fk-accent-soft)] text-[var(--fk-accent)]">
            <Plus size={14} strokeWidth={2} />
          </span>
          <span className={CARD_TITLE}>New rewrite</span>
          <span className={CARD_TEXT}>Write your own prompt.</span>
        </button>
      </div>

      {editing && (
        <TransformDialog
          initial={editing.mode === "edit" ? editing.transform : undefined}
          onSave={(d) =>
            editing.mode === "edit"
              ? act.update(editing.transform.id, d)
              : act.add(d)
          }
          onDelete={
            editing.mode === "edit" && !editing.transform.builtIn
              ? () => act.remove(editing.transform.id)
              : undefined
          }
          onClose={() => setEditing(null)}
        />
      )}
      {howOpen && <HowItWorks onClose={() => setHowOpen(false)} />}
    </div>
  );
}
