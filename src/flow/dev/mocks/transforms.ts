import type { Transform, TransformSettings } from "@/bindings";
import type { MockHandler } from "../mockTauri";

const DEFAULTS: Transform[] = [
  {
    id: "polish",
    name: "Tighten",
    description: "Shorter and clearer, same meaning",
    prompt: "Rewrite the text so it is clear and concise. Return only the rewritten text.",
    shortcut_index: 1,
  },
  {
    id: "prompt_engineer",
    name: "As a prompt",
    description: "Turns rough notes into a prompt for an AI tool",
    prompt: "Turn the text into a well-structured prompt for an AI assistant. Return only the prompt.",
    shortcut_index: 2,
  },
];

let settings: TransformSettings = {
  opt_in: true,
  auto_apply: { enabled: false, transform_id: "polish" },
  items: DEFAULTS,
};
let nextId = 100;

const ok = () => settings;

export const handlers: Record<string, MockHandler> = {
  get_transforms: () => settings,
  save_transform: ({ id, name, description, prompt, shortcutIndex }) => {
    const shortcut =
      typeof shortcutIndex === "number"
        ? shortcutIndex
        : id
          ? null
          : ([1, 2, 3, 4, 5, 6, 7, 8, 9].find(
              (n) => !settings.items.some((t) => t.shortcut_index === n),
            ) ?? null);
    const item: Transform = {
      id: typeof id === "string" ? id : String(nextId++),
      name: String(name),
      description: String(description),
      prompt: String(prompt),
      shortcut_index: shortcut,
    };
    settings = {
      ...settings,
      items: id
        ? settings.items.map((t) => (t.id === id ? item : t))
        : [...settings.items, item],
    };
    return ok();
  },
  delete_transform: ({ id }) => {
    settings = { ...settings, items: settings.items.filter((t) => t.id !== id) };
    return ok();
  },
  reset_transforms: () => {
    settings = { ...settings, items: DEFAULTS, auto_apply: { enabled: false, transform_id: null } };
    return ok();
  },
  set_transforms_opt_in: ({ enabled }) => {
    settings = { ...settings, opt_in: !!enabled };
    return ok();
  },
  set_auto_apply: ({ enabled, transformId }) => {
    settings = {
      ...settings,
      auto_apply: { enabled: !!enabled, transform_id: (transformId as string | null) ?? null },
    };
    return ok();
  },
};
