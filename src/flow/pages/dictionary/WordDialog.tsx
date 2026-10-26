import { useState, type FormEvent } from "react";
import { ModalShell } from "../../overlays/ModalShell";
import { Button, TextInput, Toggle } from "../../ui";
import { APP_NAME } from "@/flow/brand";
import {
  MAX_WORD_LENGTH,
  normalizeWord,
  type EntryInput,
} from "./useDictionary";

const Label = ({ children }: { children: string }) => (
  <span className="mb-[8px] block text-[13px] font-semibold leading-[18px] text-[var(--fk-text-muted)]">
    {children}
  </span>
);

export const WordDialog = ({
  initial,
  startAsReplacement = false,
  onSave,
  onClose,
}: {
  initial?: EntryInput;
  startAsReplacement?: boolean;
  /** Resolves to an error message, or null when saved. */
  onSave: (input: EntryInput) => Promise<string | null>;
  onClose: () => void;
}) => {
  const [word, setWord] = useState(initial?.word ?? "");
  const [isReplacement, setIsReplacement] = useState(
    initial ? initial.replacement !== undefined : startAsReplacement,
  );
  const [replacement, setReplacement] = useState(initial?.replacement ?? "");
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const cleanWord = normalizeWord(word);
  const canSave =
    cleanWord.length > 0 &&
    cleanWord.length <= MAX_WORD_LENGTH &&
    (!isReplacement || replacement.trim().length > 0);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    if (!canSave || saving) return;
    setSaving(true);
    const failed = await onSave(
      isReplacement ? { word: cleanWord, replacement } : { word: cleanWord },
    );
    setSaving(false);
    if (failed) setError(failed.endsWith(".") ? failed : `${failed}.`);
    else onClose();
  };

  return (
    <ModalShell
      onClose={onClose}
      width={440}
      radius={20}
      style={{ background: "var(--fk-menu-bg)", boxShadow: "var(--fk-elev-3)" }}
    >
      <form onSubmit={submit} className="p-[24px]">
        <h2 className="fk-display text-[22px] leading-[30px]">
          {initial ? "Edit word" : "Add word"}
        </h2>
        <p className="mt-[4px] text-[14.5px] leading-[21px] text-[var(--fk-text-muted)]">
          {isReplacement
            ? `When ${APP_NAME} hears the first, it writes the second.`
            : `${APP_NAME} will spell it this way every time.`}
        </p>
        <div className="mt-[24px]">
          <Label>{isReplacement ? "When you say" : "Word or name"}</Label>
          <TextInput
            className="!bg-[var(--fk-surface-2)] focus-within:!bg-[var(--fk-panel)]"
            autoFocus
            aria-label={isReplacement ? "When you say" : "Word or name"}
            value={word}
            maxLength={MAX_WORD_LENGTH}
            placeholder={isReplacement ? "e.g. k8s" : "e.g. Ananya Iyer"}
            onChange={(e) => {
              setWord(e.target.value);
              setError(null);
            }}
          />
        </div>
        {isReplacement && (
          <div className="mt-[16px]">
            <Label>{`${APP_NAME} writes`}</Label>
            <TextInput
              className="!bg-[var(--fk-surface-2)] focus-within:!bg-[var(--fk-panel)]"
              aria-label={`${APP_NAME} writes`}
              value={replacement}
              placeholder="e.g. Kubernetes"
              onChange={(e) => setReplacement(e.target.value)}
            />
          </div>
        )}
        <div className="mt-[16px] flex items-center justify-between gap-[16px] border-t border-[var(--fk-border)] pt-[16px] text-[14.5px]">
          <span>Write it as something else</span>
          <Toggle
            checked={isReplacement}
            onChange={setIsReplacement}
            label="Write it as something else"
          />
        </div>
        {error && (
          <p className="mt-[12px] text-[13px] leading-[18px] text-[var(--fk-live)]">
            {error}
          </p>
        )}
        <div className="mt-[24px] flex justify-end gap-[8px]">
          <Button variant="secondary" onClick={onClose}>
            Cancel
          </Button>
          <Button type="submit" disabled={!canSave || saving}>
            {initial ? "Save" : "Add word"}
          </Button>
        </div>
      </form>
    </ModalShell>
  );
};
