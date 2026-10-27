import type { AiApp } from "@/bindings";
import type { MockHandler } from "../mockTauri";

// The AI apps list in the preview: Claude and Cursor installed, Claude
// connected. Connect and Remove flip the sample state.
const command = "/Applications/Lipik.app/Contents/MacOS/lipik";
const app = (
  id: string,
  name: string,
  installed: boolean,
  file: string | null,
  how: AiApp["how"] = "file",
  note: string | null = null,
): AiApp => ({
  id,
  name,
  installed,
  connected: id === "claude_desktop",
  how,
  file: file && `/Users/rivaan/${file}`,
  note,
  manual: `Command: ${command}\nArguments: --mcp`,
});

let apps: AiApp[] = [
  app(
    "claude_desktop",
    "Claude",
    true,
    "Library/Application Support/Claude/claude_desktop_config.json",
    "file",
    "Quit and reopen Claude to load Lipik.",
  ),
  app("claude_code", "Claude Code", true, ".claude.json", "command"),
  app("cursor", "Cursor", true, ".cursor/mcp.json"),
  app("windsurf", "Windsurf", false, ".codeium/windsurf/mcp_config.json"),
  app(
    "vscode",
    "VS Code",
    false,
    "Library/Application Support/Code/User/mcp.json",
  ),
  app("codex", "Codex", true, ".codex/config.toml"),
  app("gemini", "Gemini CLI", false, ".gemini/settings.json"),
  app(
    "chatgpt",
    "ChatGPT",
    true,
    null,
    "copy",
    "ChatGPT can't use tools on your Mac. Open a note and use Copy as Markdown, then paste it into ChatGPT.",
  ),
];

const set = (id: unknown, connected: boolean) => {
  apps = apps.map((a) => (a.id === id ? { ...a, connected } : a));
  return apps.find((a) => a.id === id);
};

export const handlers: Record<string, MockHandler> = {
  ai_apps_status: () => apps,
  connect_ai_app: ({ id }) => set(id, true),
  disconnect_ai_app: ({ id }) => set(id, false),
};
