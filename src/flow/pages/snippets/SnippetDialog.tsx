import { useState } from "react";
import { ModalShell } from "../../overlays/ModalShell";
import { Button, TextInput } from "../../ui";
import { snippetStore, type Snippet, type SnippetDraft } from "./store";
import { APP_NAME } from "@/flow/brand";

const Label = ({ children, hint }: { children: string; hint: string }) => (
  <span className="mb-[8px] flex items-baseline justify-between text-[13px] leading-[18px]">
    <span className="font-semibold text-[var(--fk-text-muted)]">{children}</span>
    <span className="text-[var(--fk-text-faint)]">{hint}</span>
  </span>
);

export const SnippetDialog = ({
  initial,
  onSave,
  onClose,
}: {
  initial?: Snippet;
  onSave: (draft: SnippetDraft) => void;
  onClose: () => void;
}) => {
  const [trigger, setTrigger] = useState(initial?.trigger ?? "");
  const [expansion, setExpansion] = useState(initial?.expansion ?? "");
  const taken = snippetStore.hasTrigger(trigger, initial?.id);
  const valid = trigger.trim() !== "" && expansion.trim() !== "" && !taken;

  const save = () => {
    if (!valid) return;
    onSave({
      trigger: trigger.trim(),
      expansion: expansion.trim(),
      scope: initial?.scope ?? "personal",
    });
    onClose();
  };

  return (
    <ModalShell
      onClose={onClose}
      width={480}
      radius={20}
      className="p-[24px]"
      style={{ background: "var(--fk-menu-bg)", boxShadow: "var(--fk-elev-3)" }}
    >
      <form
        onSubmit={(e) => {
          e.preventDefault();
          save();
        }}
      >
        <h2 className="fk-display text-[22px] leading-[30px]">
          {initial ? "Edit snippet" : "New snippet"}
        </h2>
        <p className="mt-[4px] text-[14.5px] leading-[21px] text-[var(--fk-text-muted)]">
          Say the cue while dictating and {APP_NAME} types the full text.
        </p>
        <div className="mt-[24px]">
          <Label hint="What you say">Cue</Label>
          <TextInput
            className="!bg-[var(--fk-surface-2)] focus-within:!bg-[var(--fk-panel)]"
            autoFocus
            aria-label="Cue"
            placeholder="e.g. my address"
            value={trigger}
            onChange={(e) => setTrigger(e.target.value)}
          />
        </div>
        {taken && (
          <p className="mt-[8px] text-[13px] leading-[18px] text-[var(--fk-live)]">
            You already have a snippet with this cue.
          </p>
        )}
        <div className="mt-[16px]">
          <Label hint={`What ${APP_NAME} types`}>Full text</Label>
          <textarea
            className="block h-[128px] w-full resize-none rounded-[10px] border border-transparent bg-[var(--fk-surface-2)] px-[12px] py-[8px] text-[14.5px] leading-[21px] text-[var(--fk-text)] outline-none transition-[border-color,background-color,box-shadow] duration-[var(--fk-t-quick)] placeholder:text-[var(--fk-text-faint)] hover:border-[var(--fk-border)] focus:border-[var(--fk-accent)] focus:bg-[var(--fk-panel)] focus:shadow-[var(--fk-focus)]"
            aria-label="Full text"
            placeholder="e.g. 14 MG Road, Bengaluru 560001"
            value={expansion}
            onChange={(e) => setExpansion(e.target.value)}
          />
        </div>
        <div className="mt-[24px] flex justify-end gap-[8px]">
          <Button variant="secondary" onClick={onClose}>
            Cancel
          </Button>
          <Button type="submit" disabled={!valid}>
            Save
          </Button>
        </div>
      </form>
    </ModalShell>
  );
};
