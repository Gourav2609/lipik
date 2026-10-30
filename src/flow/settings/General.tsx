import { useEffect, useState } from "react";
import { useSettings } from "@/hooks/useSettings";
import { useOsType } from "@/hooks/useOsType";
import { useModelStore } from "@/stores/modelStore";
import { ShortcutInput } from "@/components/settings/ShortcutInput";
import {
  LANGUAGES,
  recognitionLanguage,
  supportsLanguageCode,
} from "@/lib/constants/languages";
import type {
  AutoSubmitKey,
  PasteMethod,
  ShortcutActivation,
} from "@/bindings";
import { Toggle } from "../ui";
import { SettingsPage } from "./SettingsPage";
import { Section, Select, SettingRow, SettingsCard } from "./parts";
import { SpeechEngineRows } from "./SpeechEngine";
import { APP_NAME } from "@/flow/brand";

const ACTIVATION_OPTIONS = [
  { value: "hold_or_toggle", label: "Hold or tap" },
  { value: "push_to_talk", label: "Hold only" },
  { value: "toggle", label: "Tap only" },
];

const Shortcut = ({ id }: { id: string }) => (
  <div className="fk-shortcut">
    <ShortcutInput shortcutId={id} grouped />
  </div>
);

export default function General() {
  const {
    getSetting,
    updateSetting,
    isUpdating,
    audioDevices,
    refreshAudioDevices,
  } = useSettings();
  const osType = useOsType();
  const {
    models,
    currentModel,
    initialized,
    initialize,
    downloadingModels,
    downloadProgress,
    selectModel,
    downloadModel,
    error: modelError,
  } = useModelStore();
  const [downloadFailed, setDownloadFailed] = useState("");

  useEffect(() => {
    if (!initialized) initialize();
  }, [initialized, initialize]);

  useEffect(() => {
    refreshAudioDevices();
  }, [refreshAudioDevices]);

  const model = models.find((m) => m.id === currentModel);
  const downloading = Object.keys(downloadingModels)[0];
  const progress = downloading
    ? downloadProgress[downloading]?.percentage
    : undefined;

  const chooseModel = async (id: string) => {
    const target = models.find((m) => m.id === id);
    if (!target) return;
    setDownloadFailed("");
    if (target.is_downloaded) {
      await selectModel(id);
    } else if (await downloadModel(id)) {
      await selectModel(id);
    } else {
      setDownloadFailed(`Couldn't download ${target.name}. Try again.`);
    }
  };

  const supported = model?.supported_languages ?? [];
  const detects = model?.supports_language_detection ?? true;
  const languageOptions = LANGUAGES.filter((l) =>
    l.value === "auto"
      ? detects
      : supported.length === 0 || supportsLanguageCode(supported, l.value),
  ).map((l) => ({ value: l.value, label: l.label }));
  const intent = getSetting("selected_language") || "auto";
  const language =
    intent !== "auto" &&
    languageOptions.some((o) => o.value === recognitionLanguage(intent))
      ? recognitionLanguage(intent)
      : (languageOptions[0]?.value ?? "auto");

  const mod = osType === "macos" ? "Cmd" : "Ctrl";
  const pasteOptions = [
    { value: "ctrl_v", label: `Paste (${mod}+V)` },
    ...(osType !== "macos" ? [{ value: "direct", label: "Type it out" }] : []),
    ...(osType === "windows" || osType === "linux"
      ? [
          { value: "ctrl_shift_v", label: "Paste (Ctrl+Shift+V)" },
          { value: "shift_insert", label: "Paste (Shift+Insert)" },
        ]
      : []),
    { value: "none", label: "Don't paste" },
  ];
  const pasteMethod = getSetting("paste_method") || "ctrl_v";
  const autoSubmit = getSetting("auto_submit") ?? false;
  const sendKeyOptions = [
    { value: "enter", label: "Enter" },
    ...(osType === "macos" ? [{ value: "cmd_enter", label: "Cmd+Enter" }] : []),
    { value: "ctrl_enter", label: "Ctrl+Enter" },
  ];

  const selectedMic = getSetting("selected_microphone");
  const micOptions = [
    { value: "Default", label: "System default" },
    ...audioDevices
      .filter((d) => d.name !== "Default")
      .map((d) => ({ value: d.name, label: d.name })),
  ];

  return (
    <SettingsPage
      title="Dictation"
      intro="Your key, microphone and speech model."
    >
      <Section title="Shortcuts">
        <SettingsCard>
          <SettingRow
            title="Dictate"
            description="Click it, then press the keys you want. fn works."
            control={<Shortcut id="transcribe" />}
          />
          <SettingRow
            title="Key behaviour"
            description="Hold while you speak, or tap once to start and again to stop."
            control={
              <Select
                label="Key behaviour"
                value={getSetting("shortcut_activation") || "hold_or_toggle"}
                options={ACTIVATION_OPTIONS}
                disabled={isUpdating("shortcut_activation")}
                onChange={(v) =>
                  updateSetting("shortcut_activation", v as ShortcutActivation)
                }
              />
            }
          />
          {osType !== "linux" && (
            <SettingRow
              title="Cancel"
              description="Stops the dictation and pastes nothing."
              control={<Shortcut id="cancel" />}
            />
          )}
        </SettingsCard>
      </Section>
      <Section title="Microphone">
        <SettingsCard>
          <SettingRow
            title="Microphone"
            description={`The input ${APP_NAME} listens to.`}
            control={
              <Select
                label="Microphone"
                value={
                  !selectedMic || selectedMic === "default"
                    ? "Default"
                    : selectedMic
                }
                options={micOptions}
                disabled={isUpdating("selected_microphone")}
                onChange={(v) => updateSetting("selected_microphone", v)}
              />
            }
          />
        </SettingsCard>
      </Section>
      <Section title="Speech to text">
        <SettingsCard>
          <SettingRow
            title="Model"
            description={
              progress !== undefined
                ? `Downloading… ${Math.round(progress)}%`
                : downloadFailed ||
                  modelError ||
                  "Runs on this Mac. A model downloads when you pick it."
            }
            control={
              <Select
                label="Model"
                value={currentModel}
                disabled={!!downloading}
                options={models.map((m) => ({
                  value: m.id,
                  label: m.is_downloaded
                    ? m.name
                    : `${m.name} (${Math.round(m.size_mb)} MB)`,
                }))}
                onChange={chooseModel}
              />
            }
          />
          {model?.supports_language_selection && (
            <SettingRow
              title="Language"
              description="The language you speak."
              control={
                <Select
                  label="Language"
                  value={language}
                  options={languageOptions}
                  disabled={isUpdating("selected_language")}
                  onChange={(v) => updateSetting("selected_language", v)}
                />
              }
            />
          )}
          {osType === "macos" && <SpeechEngineRows />}
        </SettingsCard>
      </Section>
      <Section title="Output">
        <SettingsCard>
          <SettingRow
            title="Paste method"
            description={`How ${APP_NAME} puts text into the app you're using.`}
            control={
              <Select
                label="Paste method"
                value={pasteMethod}
                options={pasteOptions}
                disabled={isUpdating("paste_method")}
                onChange={(v) =>
                  updateSetting("paste_method", v as PasteMethod)
                }
              />
            }
          />
          <SettingRow
            title="Add a space after each dictation"
            control={
              <Toggle
                label="Add a space after each dictation"
                checked={getSetting("append_trailing_space") ?? false}
                disabled={isUpdating("append_trailing_space")}
                onChange={(v) => updateSetting("append_trailing_space", v)}
              />
            }
          />
          <SettingRow
            title="Press Enter after dictation"
            description={
              pasteMethod === "none"
                ? "Needs a paste method other than Don't paste."
                : "Sends the message right after the text is typed, e.g. in Slack or ChatGPT."
            }
            control={
              <Toggle
                label="Press Enter after dictation"
                checked={autoSubmit}
                disabled={isUpdating("auto_submit") || pasteMethod === "none"}
                onChange={(v) => updateSetting("auto_submit", v)}
              />
            }
          />
          {autoSubmit && pasteMethod !== "none" && (
            <SettingRow
              title="Send key"
              description="Most chat apps send with Enter."
              control={
                <Select
                  label="Send key"
                  value={getSetting("auto_submit_key") || "enter"}
                  options={sendKeyOptions}
                  disabled={isUpdating("auto_submit_key")}
                  onChange={(v) =>
                    updateSetting("auto_submit_key", v as AutoSubmitKey)
                  }
                />
              }
            />
          )}
        </SettingsCard>
      </Section>
    </SettingsPage>
  );
}
