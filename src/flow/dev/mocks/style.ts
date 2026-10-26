import type { StyleSettings } from "@/bindings";
import type { MockHandler } from "../mockTauri";

let style: StyleSettings = {
  personal: "casual",
  work: "casual",
  email: "formal",
  other: "formal",
  auto_cleanup: false,
};

const ok = () => style;

export const handlers: Record<string, MockHandler> = {
  get_style_settings: () => style,
  set_style: ({ category, style: kind }) => {
    style = { ...style, [String(category)]: kind };
    return ok();
  },
  set_auto_cleanup: ({ enabled }) => {
    style = { ...style, auto_cleanup: !!enabled };
    return ok();
  },
};
