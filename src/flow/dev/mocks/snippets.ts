import type { Snippet } from "@/bindings";
import type { MockHandler } from "../mockTauri";

let snippets: Snippet[] = [
  { id: "2", trigger: "my email address", expansion: "asha.rao@example.com" },
  {
    id: "1",
    trigger: "summarize notes prompt",
    expansion:
      "Summarize these rough meeting notes into a short list of decisions, owners and next steps for the whole team to review.",
  },
];
let nextId = 100;

const ok = () => snippets;

export const handlers: Record<string, MockHandler> = {
  get_snippets: () => snippets,
  add_snippet: ({ trigger, expansion }) => {
    snippets = [
      {
        id: String(nextId++),
        trigger: String(trigger),
        expansion: String(expansion),
      },
      ...snippets,
    ];
    return ok();
  },
  update_snippet: ({ id, trigger, expansion }) => {
    snippets = snippets.map((s) =>
      s.id === id
        ? { ...s, trigger: String(trigger), expansion: String(expansion) }
        : s,
    );
    return ok();
  },
  delete_snippet: ({ id }) => {
    snippets = snippets.filter((s) => s.id !== id);
    return ok();
  },
};
