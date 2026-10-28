import { useEffect, useRef, useState, type ReactNode } from "react";
import { Check, CircleAlert, Loader2 } from "lucide-react";
import { toast } from "sonner";
import {
  commands,
  type CalendarAccess,
  type MeetingLanguage,
  type NotetakerSettings as Settings,
} from "@/bindings";
import { Button, Toggle } from "../ui";
import { SettingsPage } from "./SettingsPage";
import {
  FIELD_CLASS,
  Section,
  Select,
  SettingRow,
  SettingsCard,
  useLocalSetting,
  type SelectOption,
} from "./parts";
import { APP_NAME } from "@/flow/brand";
import {
  downloadLabel,
  openOllamaDownload,
  PullProgress,
  SLOW_MODEL_TEXT,
  useCleanupModel,
} from "../cleanupModel";
import { CALENDAR_CHANGED } from "../overlays/ConnectCalendar";
import { useFlowStore } from "../state";

const DEFAULTS: Settings = {
  detect_calls: true,
  auto_start: false,
  notify_before_seconds: 60,
  max_recording_minutes: 120,
  ollama_url: "http://127.0.0.1:11434",
  ollama_model: "qwen3:8b",
  language: "auto",
};

const NOTIFY_OPTIONS = [
  { value: "0", label: "Off" },
  { value: "15", label: "15 sec" },
  { value: "60", label: "1 min" },
  { value: "120", label: "2 min" },
  { value: "300", label: "5 min" },
  { value: "600", label: "10 min" },
];

const LANGUAGE_OPTIONS = [
  { value: "auto", label: "Auto" },
  { value: "english", label: "English" },
  { value: "hinglish", label: "Hinglish" },
];

const LANGUAGE_TEXT: Record<MeetingLanguage, string> = {
  auto: "Listens for Hindi in each meeting and keeps Hindi words and names when it hears them.",
  english: "Every meeting is transcribed as English.",
  hinglish:
    "Every meeting keeps Hindi words and names, written in Roman letters.",
};

const LENGTH_OPTIONS = [
  { value: "30", label: "30 min" },
  { value: "60", label: "1 hour" },
  { value: "120", label: "2 hours" },
  { value: "180", label: "3 hours" },
];

/** Keeps a saved value that is not in the list (older build, edited file) selectable. */
const withValue = (
  options: SelectOption[],
  value: number,
  label: (n: number) => string,
): SelectOption[] =>
  options.some((o) => o.value === String(value))
    ? options
    : [...options, { value: String(value), label: label(value) }];

const CALENDAR_TEXT: Record<CalendarAccess, string> = {
  full_access: `${APP_NAME} reads today's and tomorrow's events from the Calendar app.`,
  not_determined: `Let ${APP_NAME} read your calendar so notes get the meeting's name and you get a reminder.`,
  denied: `Access is off. Turn on ${APP_NAME} in System Settings › Privacy & Security › Calendars, then check again.`,
  restricted:
    "Calendar access is blocked on this Mac, for example by a device profile.",
  write_only: `${APP_NAME} can only add events. Allow full access in System Settings › Privacy & Security › Calendars.`,
  unsupported: "Calendar access isn't available on this computer.",
};

const useNotetakerSettings = () => {
  const [settings, setSettings] = useState<Settings | null>(null);
  const current = useRef<Settings | null>(null);

  useEffect(() => {
    commands
      .notetakerGetSettings()
      .catch(() => DEFAULTS)
      .then((s) => {
        current.current = s ?? DEFAULTS;
        setSettings(current.current);
      });
  }, []);

  const update = async (patch: Partial<Settings>) => {
    const prev = current.current;
    if (!prev) return;
    const next = { ...prev, ...patch };
    current.current = next;
    setSettings(next);
    const res = await commands.notetakerSetSettings(next);
    if (res.status === "error") {
      current.current = prev;
      setSettings(prev);
      toast(res.error);
    }
  };

  return [settings, update] as const;
};

const ModelField = ({
  value,
  disabled,
  onSave,
}: {
  value: string;
  disabled: boolean;
  onSave: (model: string) => void;
}) => {
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);

  const commit = () => {
    const model = draft.trim();
    if (!model) setDraft(value);
    else if (model !== value) onSave(model);
  };

  return (
    <input
      aria-label="AI model"
      value={draft}
      disabled={disabled}
      spellCheck={false}
      placeholder={DEFAULTS.ollama_model}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === "Enter") e.currentTarget.blur();
        if (e.key === "Escape") {
          e.preventDefault();
          setDraft(value);
        }
      }}
      className={`${FIELD_CLASS} w-[208px] px-[12px]`}
    />
  );
};

const CalendarControl = ({ access }: { access: CalendarAccess | null }) => {
  const openOverlay = useFlowStore((s) => s.openOverlay);

  if (access === "full_access") {
    return (
      <span className="flex items-center gap-[6px] text-[14.5px] font-medium text-[var(--fk-ok)]">
        <Check size={16} strokeWidth={1.75} />
        Connected
      </span>
    );
  }
  if (
    access !== "not_determined" &&
    access !== "denied" &&
    access !== "write_only"
  ) {
    return null;
  }
  return (
    <Button variant="secondary" onClick={() => openOverlay("connect-calendar")}>
      Connect
    </Button>
  );
};

const ENGINE_OPTIONS = [
  { value: "builtin", label: "Built-in (recommended)" },
  { value: "ollama", label: "Ollama (advanced)" },
];

const CleanupModelRow = ({
  view,
  engine,
  model,
  size,
  progress,
  error,
  download,
  cancel,
}: ReturnType<typeof useCleanupModel>) => {
  const builtin = engine === "builtin";
  const row = (description: ReactNode, control?: ReactNode) => (
    <SettingRow
      title="Clean-up model"
      description={description}
      control={control}
    />
  );
  switch (view) {
    case "checking":
      return row(
        "Checking the clean-up model…",
        <Loader2
          size={16}
          strokeWidth={1.75}
          className="animate-spin text-[var(--fk-text-muted)]"
        />,
      );
    case "ready":
      return row(
        builtin
          ? `${model} is ready. It runs inside ${APP_NAME}, on this Mac.`
          : `${model} is ready in Ollama.`,
        <span className="flex items-center gap-[6px] text-[14.5px] font-medium text-[var(--fk-ok)]">
          <Check size={16} strokeWidth={1.75} />
          Ready
        </span>,
      );
    case "no_ollama":
      return row(
        "Ollama isn't running. Clean-up and notes need this free app. If you have it already, open it.",
        <Button variant="secondary" onClick={openOllamaDownload}>
          Get Ollama (free)
        </Button>,
      );
    case "no_model":
      return row(
        builtin
          ? `Clean-up needs ${model}, a model that runs on this Mac. It downloads once.`
          : `Ollama is running but doesn't have ${model} yet.`,
        <Button variant="secondary" onClick={download}>
          {downloadLabel(model, size)}
        </Button>,
      );
    case "slow_model":
      return row(
        <span className="flex items-start gap-[6px]">
          <CircleAlert
            size={14}
            strokeWidth={1.75}
            className="mt-[2px] shrink-0 text-[var(--fk-live)]"
          />
          {SLOW_MODEL_TEXT}
        </span>,
      );
    case "downloading":
      return (
        <div className="border-b border-[var(--fk-border)] py-[16px] last:border-b-0">
          <div className="mb-[8px] text-[15px] font-medium leading-[22px]">
            Downloading {model}
          </div>
          <PullProgress progress={progress} onCancel={cancel} />
        </div>
      );
    case "verifying":
      return row(
        `Checking the download of ${model}…`,
        <Loader2
          size={16}
          strokeWidth={1.75}
          className="animate-spin text-[var(--fk-text-muted)]"
        />,
      );
    case "failed":
      return row(
        <span className="flex items-start gap-[6px]">
          <CircleAlert
            size={14}
            strokeWidth={1.75}
            className="mt-[2px] shrink-0 text-[var(--fk-live)]"
          />
          The download didn't finish. {error}
        </span>,
        <Button variant="secondary" onClick={download}>
          Try again
        </Button>,
      );
  }
};

export default function NotetakerSettings() {
  const [settings, update] = useNotetakerSettings();
  const [calendar, setCalendar] = useState<CalendarAccess | null>(null);
  const [menuBar, setMenuBar] = useLocalSetting("notetaker.menuBar", true);

  useEffect(() => {
    const read = () =>
      commands
        .notetakerCalendarAccess()
        .then(setCalendar)
        .catch(() => setCalendar("unsupported"));
    read();
    window.addEventListener(CALENDAR_CHANGED, read);
    return () => window.removeEventListener(CALENDAR_CHANGED, read);
  }, []);

  const cleanup = useCleanupModel();
  const s = settings ?? DEFAULTS;
  const loading = settings === null;

  return (
    <SettingsPage
      title="Meetings"
      intro="Notes from your calls, written as you talk."
    >
      <Section title="Calls">
        <SettingsCard>
          <SettingRow
            title="Reminder before a meeting"
            description="When to remind you about a meeting on your calendar."
            control={
              <Select
                label="Reminder before a meeting"
                value={String(s.notify_before_seconds)}
                options={withValue(
                  NOTIFY_OPTIONS,
                  s.notify_before_seconds,
                  (n) => (n % 60 ? `${n} sec` : `${n / 60} min`),
                )}
                disabled={loading}
                onChange={(v) => update({ notify_before_seconds: Number(v) })}
              />
            }
          />
          <SettingRow
            title="Show your next meeting in the menu bar"
            description={`Its name and a countdown sit next to the ${APP_NAME} icon. Off keeps the icon and its menu.`}
            control={
              <Toggle
                label="Show your next meeting in the menu bar"
                checked={menuBar}
                onChange={setMenuBar}
              />
            }
          />
          <SettingRow
            title="Notice calls"
            description={`${APP_NAME} asks whether to take notes when you join a call.`}
            control={
              <Toggle
                label="Notice calls"
                checked={s.detect_calls}
                disabled={loading}
                onChange={(on) => update({ detect_calls: on })}
              />
            }
          />
          <SettingRow
            title="Start notes automatically"
            description={
              s.detect_calls
                ? "Notes start when a meeting begins."
                : "Turn on Notice calls to use this."
            }
            control={
              <Toggle
                label="Start notes automatically"
                checked={s.detect_calls && s.auto_start}
                disabled={loading || !s.detect_calls}
                onChange={(on) => update({ auto_start: on })}
              />
            }
          />
          <SettingRow
            title="Meeting language"
            description={LANGUAGE_TEXT[s.language]}
            control={
              <Select
                label="Meeting language"
                value={s.language}
                options={LANGUAGE_OPTIONS}
                disabled={loading}
                onChange={(v) => update({ language: v as MeetingLanguage })}
              />
            }
          />
          <SettingRow
            title="Longest recording"
            description="Recording stops on its own after this long."
            control={
              <Select
                label="Longest recording"
                value={String(s.max_recording_minutes)}
                options={withValue(
                  LENGTH_OPTIONS,
                  s.max_recording_minutes,
                  (n) => `${n} min`,
                )}
                disabled={loading}
                onChange={(v) => update({ max_recording_minutes: Number(v) })}
              />
            }
          />
        </SettingsCard>
      </Section>
      <Section title="Calendar">
        <SettingsCard>
          <SettingRow
            title="Calendar access"
            description={calendar ? CALENDAR_TEXT[calendar] : undefined}
            control={<CalendarControl access={calendar} />}
          />
        </SettingsCard>
      </Section>
      <Section title="Notes">
        <SettingsCard>
          {cleanup.builtinAvailable && (
            <SettingRow
              title="Clean-up engine"
              description={
                cleanup.engine === "ollama"
                  ? "Clean-up and notes use the AI model below, in Ollama, a separate app."
                  : `Clean-up and notes run inside ${APP_NAME}, on this Mac.`
              }
              control={
                <Select
                  label="Clean-up engine"
                  value={cleanup.engine ?? "builtin"}
                  options={ENGINE_OPTIONS}
                  width={240}
                  disabled={cleanup.engine === null}
                  onChange={(v) =>
                    cleanup.setEngine(v === "ollama" ? "ollama" : "builtin")
                  }
                />
              }
            />
          )}
          <CleanupModelRow {...cleanup} />
          <SettingRow
            title="AI model"
            description={
              cleanup.engine === "builtin"
                ? "Used only when the engine is Ollama."
                : "The Ollama model that cleans up your dictation and writes your notes."
            }
            control={
              <ModelField
                value={s.ollama_model}
                disabled={loading}
                onSave={(model) =>
                  update({ ollama_model: model }).then(cleanup.recheck)
                }
              />
            }
          />
          <SettingRow
            title="Meeting audio"
            description="Recordings stay on this Mac until you delete the meeting."
            control={
              <Select
                label="Meeting audio"
                value="forever"
                options={[{ value: "forever", label: "Keep all" }]}
                onChange={() => {}}
                disabled
              />
            }
          />
        </SettingsCard>
      </Section>
    </SettingsPage>
  );
}
