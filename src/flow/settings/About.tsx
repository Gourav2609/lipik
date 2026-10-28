import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";
import { Button, LipikMark } from "../ui";
import { runSetupAgain } from "../onboarding/runAgain";
import { useFlowStore } from "../state";
import { SettingsPage } from "./SettingsPage";
import { Section, SettingRow, SettingsCard } from "./parts";
import { APP_NAME } from "@/flow/brand";

const Value = ({ children }: { children: string }) => (
  <span className="fk-num text-[14.5px] leading-[22px] text-[var(--fk-text-muted)]">
    {children}
  </span>
);

export default function About() {
  const closeSettings = useFlowStore((s) => s.closeSettings);
  const [version, setVersion] = useState("");
  useEffect(() => {
    getVersion()
      .then(setVersion)
      .catch(() => setVersion(""));
  }, []);

  return (
    <SettingsPage title="About" intro={`${APP_NAME} is free and open source.`}>
      <div className="flex items-center gap-[16px] rounded-[var(--fk-r-lg)] border-l-[3px] border-[var(--fk-accent)] bg-[var(--fk-accent-soft)] px-[20px] py-[20px]">
        <LipikMark size={48} tile />
        <div>
          <div className="fk-display text-[22px] leading-[30px]">
            {APP_NAME}{" "}
            <span className="text-[var(--fk-text-muted)]">लिपिक</span>
          </div>
          <div className="text-[14.5px] leading-[21px] text-[var(--fk-text-muted)]">
            Speak. {APP_NAME} writes it down.
          </div>
        </div>
      </div>
      <Section title="This copy">
        <SettingsCard>
          <SettingRow
            title="Version"
            control={<Value>{version || "Unknown"}</Value>}
          />
          <SettingRow title="Licence" control={<Value>MIT</Value>} />
          <SettingRow
            title="Setup"
            description="Go through the first-run steps again. Your settings stay as they are."
            control={
              <Button
                variant="secondary"
                size="sm"
                onClick={() => {
                  closeSettings();
                  runSetupAgain();
                }}
              >
                Run setup again
              </Button>
            }
          />
        </SettingsCard>
      </Section>
    </SettingsPage>
  );
}
