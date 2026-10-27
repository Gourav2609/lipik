import { useEffect, useState } from "react";
import { toast } from "sonner";
import { commands, type RecordingRetentionPeriod } from "@/bindings";
import { useSettings } from "@/hooks/useSettings";
import { Button, Toggle } from "../ui";
import { SettingsPage } from "./SettingsPage";
import { Section, Select, SettingRow, SettingsCard } from "./parts";
import { APP_NAME } from "@/flow/brand";

const HISTORY_OPTIONS = [10, 25, 50, 100, 250, 500, 1000].map((n) => ({
  value: String(n),
  label: `Last ${n}`,
}));

// The backend's serde names are "days3" etc.; the generated type says "days_3".
// "never" is the older name for "forever"; both keep every recording.
const RETENTION_OPTIONS = [
  { value: "forever", label: "Keep all" },
  { value: "preserve_limit", label: "Same as history" },
  { value: "days3", label: "3 days" },
  { value: "weeks2", label: "2 weeks" },
  { value: "months3", label: "3 months" },
];

const OpenButton = ({ onClick }: { onClick: () => void }) => (
  <Button variant="secondary" onClick={onClick}>
    Open
  </Button>
);

/** Voices kept from speakers the user named in meeting notes. */
const SavedVoices = () => {
  const [count, setCount] = useState<number | null>(null);
  useEffect(() => {
    void commands
      .notetakerSavedVoices()
      .then((r) => setCount(r.status === "ok" ? r.data : null))
      .catch(() => setCount(null));
  }, []);
  const forget = async () => {
    const r = await commands.notetakerForgetVoices().catch(() => null);
    if (r?.status === "ok") setCount(0);
    else toast(`Couldn't forget the voices: ${r?.error ?? "try again"}`);
  };
  const people =
    count === null ? "" : `: ${count} ${count === 1 ? "person" : "people"}`;
  return (
    <SettingRow
      title={`Saved voices${people}`}
      description="When you name a speaker in meeting notes, their voice is kept on this Mac so later meetings can name them too."
      control={
        <Button variant="secondary" disabled={!count} onClick={forget}>
          Forget all voices
        </Button>
      }
    />
  );
};

export default function Privacy() {
  const { getSetting, updateSetting, isUpdating } = useSettings();
  const history = String(getSetting("history_limit") ?? 100);
  const retention = getSetting("recording_retention_period") || "forever";

  return (
    <SettingsPage
      title="Privacy"
      intro={`${APP_NAME} turns speech into text on this Mac. Recordings and history stay in the folders below.`}
    >
      <Section title="History">
        <SettingsCard>
          <SettingRow
            title="Dictation history"
            description="How many dictations to keep."
            control={
              <Select
                label="Dictation history"
                value={
                  HISTORY_OPTIONS.some((o) => o.value === history)
                    ? history
                    : HISTORY_OPTIONS[3].value
                }
                options={HISTORY_OPTIONS}
                disabled={isUpdating("history_limit")}
                onChange={(v) => updateSetting("history_limit", Number(v))}
              />
            }
          />
          <SettingRow
            title="Audio recordings"
            description="Delete recordings after this long."
            control={
              <Select
                label="Audio recordings"
                value={retention === "never" ? "forever" : retention}
                options={RETENTION_OPTIONS}
                disabled={isUpdating("recording_retention_period")}
                onChange={(v) =>
                  updateSetting(
                    "recording_retention_period",
                    v as RecordingRetentionPeriod,
                  )
                }
              />
            }
          />
        </SettingsCard>
      </Section>
      <Section title="Updates">
        <SettingsCard>
          <SettingRow
            title="Check for new versions"
            description="Once a day, asks GitHub for the latest version number. Nothing about you is sent."
            control={
              <Toggle
                label="Check for new versions"
                checked={getSetting("release_check_enabled") ?? true}
                disabled={isUpdating("release_check_enabled")}
                onChange={(v) => updateSetting("release_check_enabled", v)}
              />
            }
          />
        </SettingsCard>
      </Section>
      <Section title="Your data">
        <SettingsCard>
          <SettingRow
            title="Data folder"
            description="Settings, models and history."
            control={<OpenButton onClick={() => commands.openAppDataDir()} />}
          />
          <SettingRow
            title="Recordings folder"
            description="The audio of each dictation."
            control={
              <OpenButton onClick={() => commands.openRecordingsFolder()} />
            }
          />
          <SavedVoices />
        </SettingsCard>
      </Section>
    </SettingsPage>
  );
}
