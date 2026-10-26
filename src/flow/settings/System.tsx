import type { ReactNode } from "react";
import { useSettings } from "@/hooks/useSettings";
import { Toggle } from "../ui";
import { SettingsPage } from "./SettingsPage";
import { Section, SettingRow, SettingsCard, useLocalSetting } from "./parts";

export default function System() {
  const { getSetting, updateSetting, isUpdating } = useSettings();
  const [alwaysShowBar, setAlwaysShowBar] = useLocalSetting(
    "alwaysShowBar",
    true,
  );
  const [showInDock, setShowInDock] = useLocalSetting("showInDock", true);
  const [notifyTips, setNotifyTips] = useLocalSetting("notifyTips", true);

  const backendToggle = (
    key:
      | "autostart_enabled"
      | "audio_feedback"
      | "mute_while_recording",
    label: string,
  ) => (
    <Toggle
      label={label}
      checked={getSetting(key) ?? false}
      disabled={isUpdating(key)}
      onChange={(v) => updateSetting(key, v)}
    />
  );

  const row = (label: string, control: ReactNode) => (
    <SettingRow title={label} control={control} />
  );

  return (
    <SettingsPage title="App" intro="The pill, the Dock, sounds and notifications.">
      <Section title="Start up">
        <SettingsCard>
          {row(
            "Open at login",
            backendToggle("autostart_enabled", "Open at login"),
          )}
          {row(
            "Always show the pill",
            <Toggle
              label="Always show the pill"
              checked={alwaysShowBar}
              onChange={setAlwaysShowBar}
            />,
          )}
          {row(
            "Show in the Dock",
            <Toggle
              label="Show in the Dock"
              checked={showInDock}
              onChange={setShowInDock}
            />,
          )}
        </SettingsCard>
      </Section>
      <Section title="Sound">
        <SettingsCard>
          {row(
            "Sounds for dictation and alerts",
            backendToggle(
              "audio_feedback",
              "Sounds for dictation and alerts",
            ),
          )}
          {row(
            "Mute other audio while you speak",
            backendToggle(
              "mute_while_recording",
              "Mute other audio while you speak",
            ),
          )}
        </SettingsCard>
      </Section>
      <Section title="Notifications">
        <SettingsCard>
          {row(
            "Show tips",
            <Toggle
              label="Show tips"
              checked={notifyTips}
              onChange={setNotifyTips}
            />,
          )}
        </SettingsCard>
      </Section>
    </SettingsPage>
  );
}
