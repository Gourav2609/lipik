import { Button, Toggle } from "../ui";
import { SettingsPage } from "./SettingsPage";
import { SettingRow, SettingsCard, useLocalSetting } from "./parts";

export default function VibeCoding() {
  const [fileTagging, setFileTagging] = useLocalSetting(
    "vibe.fileTagging",
    true,
  );

  return (
    <SettingsPage title="Code editors" intro="Dictate into your code editor.">
      <SettingsCard>
        <SettingRow
          title="Names from your code"
          description="Spells variables and functions the way your code does. VS Code, Cursor and Windsurf."
          control={
            <Button variant="secondary" title="Not available yet">
              Set up
            </Button>
          }
        />
        <SettingRow
          title="File tags in chat"
          description={
            <>
              Say a file name, like{" "}
              <code className="rounded-[var(--fk-r-xs)] bg-[var(--fk-surface-2)] px-[4px] font-mono text-[12px] text-[var(--fk-text)]">
                index.tsx
              </code>
              , and it becomes a file tag. Cursor and Windsurf.
            </>
          }
          control={
            <Toggle
              label="File tags in chat"
              checked={fileTagging}
              onChange={setFileTagging}
            />
          }
        />
      </SettingsCard>
    </SettingsPage>
  );
}
