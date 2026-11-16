import { useId, useState } from "react";
import { Pencil, Plus, Trash2 } from "lucide-react";
import type { NamedText } from "@/bindings";
import { Button, IconButton, TextInput } from "../ui";

const TEXT_CLASS =
  "block h-[96px] w-full resize-none rounded-[10px] border border-transparent bg-[var(--fk-panel)] px-[12px] py-[10px] text-[14.5px] leading-[21px] text-[var(--fk-text)] outline-none transition-[border-color,box-shadow] duration-[var(--fk-t-quick)] placeholder:text-[var(--fk-text-faint)] hover:border-[var(--fk-border)] focus:border-[var(--fk-accent)] focus:shadow-[var(--fk-focus)]";

const Editor = ({
  initial,
  namePlaceholder,
  textPlaceholder,
  onSave,
  onCancel,
}: {
  initial?: NamedText;
  namePlaceholder: string;
  textPlaceholder: string;
  onSave: (name: string, text: string) => void;
  onCancel: () => void;
}) => {
  const [name, setName] = useState(initial?.name ?? "");
  const [text, setText] = useState(initial?.text ?? "");
  const valid = name.trim() !== "" && text.trim() !== "";
  const id = useId();

  return (
    <form
      className="border-b border-[var(--fk-border)] py-[16px] last:border-b-0"
      onSubmit={(e) => {
        e.preventDefault();
        if (valid) onSave(name.trim(), text.trim());
      }}
    >
      <TextInput
        id={`${id}-name`}
        autoFocus
        aria-label="Name"
        placeholder={namePlaceholder}
        value={name}
        onChange={(e) => setName(e.target.value)}
      />
      <textarea
        id={`${id}-text`}
        aria-label="What it asks for"
        className={`${TEXT_CLASS} mt-[8px]`}
        placeholder={textPlaceholder}
        value={text}
        onChange={(e) => setText(e.target.value)}
      />
      <div className="mt-[12px] flex justify-end gap-[8px]">
        <Button variant="secondary" size="sm" onClick={onCancel}>
          Cancel
        </Button>
        <Button type="submit" size="sm" disabled={!valid}>
          Save
        </Button>
      </div>
    </form>
  );
};

/** A list of named texts (templates, saved prompts) edited in place. */
export const NamedTexts = ({
  items,
  addLabel,
  namePlaceholder,
  textPlaceholder,
  disabled = false,
  onChange,
}: {
  items: NamedText[];
  addLabel: string;
  namePlaceholder: string;
  textPlaceholder: string;
  disabled?: boolean;
  onChange: (items: NamedText[]) => void;
}) => {
  const [editing, setEditing] = useState<string | null>(null);

  const save = (id: string | null, name: string, text: string) => {
    onChange(
      id
        ? items.map((i) => (i.id === id ? { ...i, name, text } : i))
        : [...items, { id: `custom-${Date.now().toString(36)}`, name, text }],
    );
    setEditing(null);
  };

  return (
    <div>
      {items.map((item) =>
        editing === item.id ? (
          <Editor
            key={item.id}
            initial={item}
            namePlaceholder={namePlaceholder}
            textPlaceholder={textPlaceholder}
            onSave={(name, text) => save(item.id, name, text)}
            onCancel={() => setEditing(null)}
          />
        ) : (
          <div
            key={item.id}
            className="flex min-h-[64px] items-center gap-[16px] border-b border-[var(--fk-border)] py-[12px] last:border-b-0"
          >
            <div className="min-w-0 flex-1">
              <div className="text-[15px] font-medium leading-[22px]">
                {item.name}
              </div>
              <div className="mt-[2px] line-clamp-2 text-[13px] leading-[18px] text-[var(--fk-text-muted)]">
                {item.text}
              </div>
            </div>
            <div className="flex shrink-0 items-center gap-[2px]">
              <IconButton
                label={`Edit ${item.name}`}
                disabled={disabled}
                onClick={() => setEditing(item.id)}
              >
                <Pencil size={15} strokeWidth={1.75} />
              </IconButton>
              <IconButton
                label={`Delete ${item.name}`}
                disabled={disabled}
                onClick={() => onChange(items.filter((i) => i.id !== item.id))}
              >
                <Trash2 size={15} strokeWidth={1.75} />
              </IconButton>
            </div>
          </div>
        ),
      )}
      {editing === "new" ? (
        <Editor
          namePlaceholder={namePlaceholder}
          textPlaceholder={textPlaceholder}
          onSave={(name, text) => save(null, name, text)}
          onCancel={() => setEditing(null)}
        />
      ) : (
        <div className="py-[12px]">
          <Button
            variant="ghost"
            size="sm"
            disabled={disabled}
            icon={<Plus size={14} strokeWidth={1.75} />}
            onClick={() => setEditing("new")}
          >
            {addLabel}
          </Button>
        </div>
      )}
    </div>
  );
};
