import { SettingsPage } from "./SettingsPage";
import { Section } from "./parts";
import { APP_NAME } from "@/flow/brand";
import { AiAppRows, OtherApps, useAiApps } from "../aiApps";

export default function Mcp() {
  const state = useAiApps();
  return (
    <SettingsPage
      title="AI apps"
      intro={`Let AI apps read your ${APP_NAME} meeting notes, on this Mac, through MCP. They can't see your dictations, and they never change anything.`}
    >
      <AiAppRows state={state} />
      <Section title="Other apps">
        <OtherApps apps={state.apps} />
      </Section>
    </SettingsPage>
  );
}
