import { useId, useState } from "react";
import { X } from "lucide-react";
import { ModalShell } from "../../overlays/ModalShell";
import { Button, IconButton, TextInput } from "../../ui";
import type { Transform, TransformDraft } from "./store";
import { confirmAction } from "./confirm";
import { APP_NAME } from "@/flow/brand";

const LABEL = "block text-[13px] font-semibold leading-[18px] text-[var(--fk-text-muted)]";

export const TransformDialog = ({
  initial,
  onSave,
  onDelete,
  onClose,
}: {
  initial?: Transform;
  onSave: (draft: TransformDraft) => void;
  onDelete?: () => void;
  onClose: () => void;
}) => {
  const [name, setName] = useState(initial?.name ?? "");
  const [description, setDescription] = useState(initial?.description ?? "");
  const [prompt, setPrompt] = useState(initial?.prompt ?? "");
  const valid = name.trim() !== "" && prompt.trim() !== "";
  const dirty =
    name !== (initial?.name ?? "") ||
    description !== (initial?.description ?? "") ||
    prompt !== (initial?.prompt ?? "");
  const id = useId();

  const dismiss = async () => {
    if (
      !dirty ||
      (await confirmAction("Discard your changes?", "Discard changes"))
    )
      onClose();
  };

  return (
    <ModalShell
      onClose={dismiss}
      width={520}
      className="p-[24px]"
    >
      <form
        onSubmit={(e) => {
          e.preventDefault();
          if (!valid) return;
          onSave({
            name: name.trim(),
            description: description.trim(),
            prompt: prompt.trim(),
          });
          onClose();
        }}
      >
        <div className="flex items-center justify-between">
          <h2 className="fk-display text-[22px] leading-[30px]">
            {initial ? "Edit rewrite" : "New rewrite"}
          </h2>
          <IconButton label="Close" onClick={dismiss}>
            <X size={18} strokeWidth={1.75} />
          </IconButton>
        </div>
        <label htmlFor={`${id}-name`} className={`${LABEL} mt-[20px]`}>
          Name
        </label>
        <TextInput
          id={`${id}-name`}
          className="mt-[8px]"
          autoFocus
          placeholder="Friendly reply"
          value={name}
          onChange={(e) => setName(e.target.value)}
        />
        <label htmlFor={`${id}-description`} className={`${LABEL} mt-[16px]`}>
          Description
        </label>
        <TextInput
          id={`${id}-description`}
          className="mt-[8px]"
          placeholder="What this rewrite does"
          value={description}
          onChange={(e) => setDescription(e.target.value)}
        />
        <label htmlFor={`${id}-prompt`} className={`${LABEL} mt-[16px]`}>
          Prompt
        </label>
        <textarea
          id={`${id}-prompt`}
          className="mt-[8px] block h-[140px] w-full resize-none rounded-[10px] border border-transparent bg-[var(--fk-surface)] px-[12px] py-[10px] text-[14.5px] leading-[21px] text-[var(--fk-text)] outline-none transition-[border-color,background-color,box-shadow] duration-[var(--fk-t-quick)] placeholder:text-[var(--fk-text-faint)] hover:border-[var(--fk-border)] focus:border-[var(--fk-accent)] focus:bg-[var(--fk-panel)] focus:shadow-[var(--fk-focus)]"
          placeholder={`Tell ${APP_NAME} how to rewrite the text`}
          value={prompt}
          onChange={(e) => setPrompt(e.target.value)}
        />
        <div className="mt-[24px] flex items-center gap-[8px]">
          {onDelete && (
            <Button
              variant="danger"
              onClick={async () => {
                if (
                  !(await confirmAction(
                    `Delete "${initial?.name ?? ""}"? This can't be undone.`,
                    "Delete rewrite",
                  ))
                )
                  return;
                onDelete();
                onClose();
              }}
            >
              Delete
            </Button>
          )}
          <span className="flex-1" />
          <Button variant="secondary" onClick={dismiss}>
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
