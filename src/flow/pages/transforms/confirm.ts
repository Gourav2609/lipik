import { ask } from "@tauri-apps/plugin-dialog";
import { isFlowPreview } from "../../dev/preview";

export const confirmAction = (message: string, title: string) =>
  isFlowPreview
    ? Promise.resolve(window.confirm(message))
    : ask(message, { title, kind: "warning" });
