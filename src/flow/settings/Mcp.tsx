import { SettingsPage } from "./SettingsPage";
import { AppList, type AppEntry } from "./apps";
import { Section } from "./parts";
import { APP_NAME } from "@/flow/brand";

const app = (id: string, name: string): AppEntry => ({
  id,
  name,
  description: `Let ${name} read your meeting notes and transcripts.`,
  action: "Connect",
});

const APPS = [
  app("claude", "Claude"),
  app("chatgpt", "ChatGPT"),
  app("gemini", "Gemini"),
  app("cursor", "Cursor"),
];

export default function Mcp() {
  return (
    <SettingsPage
      title="AI tools"
      intro="Let AI apps read your meeting notes through MCP. They can't see your dictations."
    >
      <AppList storageKey="mcp.connected" apps={APPS} initial={["claude"]} />
      <Section title="Other apps">
        <p className="text-[14.5px] leading-[21px] text-[var(--fk-text-muted)]">
          Any app that supports MCP can connect once the {APP_NAME} MCP server
          is ready.
        </p>
      </Section>
    </SettingsPage>
  );
}
