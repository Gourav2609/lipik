import "../settings/settings.css";
import {
  Fragment,
  useEffect,
  useRef,
  useState,
  type ComponentType,
  type ReactNode,
} from "react";
import {
  Check,
  CircleAlert,
  Code,
  Keyboard,
  Loader2,
  Mail,
  MessageSquare,
  Mic,
  NotebookPen,
  ShieldCheck,
} from "lucide-react";
import { events } from "@/bindings";
import { ShortcutInput } from "@/components/settings/ShortcutInput";
import { useSettings } from "@/hooks/useSettings";
import { APP_NAME } from "../brand";
import { isFlowPreview } from "../dev/preview";
import { Select } from "../settings/parts";
import { entryText } from "../pages/dictation/useHistory";
import {
  downloadLabel,
  openOllamaDownload,
  PullProgress,
  SLOW_MODEL_TEXT,
  useCleanupModel,
} from "../cleanupModel";
import { AiAppRows, useAiApps } from "../aiApps";
import { Button, Card, Kbd } from "../ui";
import { retryDefaultModelSetup, useModelSetup } from "./defaultModel";
import { StepLayout, type StepProps } from "./StepLayout";
import { useMicLevel } from "./useMicLevel";
import { usePermissions, type PermissionKind } from "./usePermissions";
import {
  AiAppsVisual,
  CleanupVisual,
  KeyboardVisual,
  MarkVisual,
  MeterVisual,
  PillPreview,
  WritingVisual,
} from "./visuals";

const Continue = ({
  next,
  label = "Continue",
  disabled,
}: {
  next: () => void;
  label?: string;
  disabled?: boolean;
}) => (
  <Button onClick={next} disabled={disabled} className="min-w-[120px]">
    {label}
  </Button>
);

const LinkButton = ({
  onClick,
  children,
}: {
  onClick: () => void;
  children: ReactNode;
}) => (
  <Button
    variant="ghost"
    onClick={onClick}
    className="!font-medium !text-[var(--fk-text-muted)]"
  >
    {children}
  </Button>
);

const OkNote = ({ children }: { children: ReactNode }) => (
  <span className="flex items-center gap-[6px] text-[14px] leading-[20px] text-[var(--fk-ok)]">
    <Check size={16} strokeWidth={1.75} />
    {children}
  </span>
);

const keyLabel = (binding: string | undefined) =>
  (binding ?? "fn")
    .split("+")
    .map((k) => (k.length <= 2 ? k : k[0].toUpperCase() + k.slice(1)))
    .join(" + ");

const Keys = ({ label, size }: { label: string; size?: "md" | "lg" }) => (
  <span className="inline-flex items-center gap-[4px] align-middle">
    {label.split(" + ").map((k, i) => (
      <Fragment key={k}>
        {i > 0 && <span className="text-[var(--fk-text-faint)]">+</span>}
        <Kbd size={size} className="![font-family:var(--fk-font-sans)]">
          {k}
        </Kbd>
      </Fragment>
    ))}
  </span>
);

const useHotkey = () => {
  const { settings } = useSettings();
  const binding = settings?.bindings?.transcribe?.current_binding ?? "fn";
  return { binding, key: keyLabel(binding) };
};

/* ---------- welcome: where will you use it ---------- */

type UseCase = "email" | "chat" | "notes" | "code";

const USE_CASES: { id: UseCase; label: string; Icon: typeof Mail }[] = [
  { id: "email", label: "Email", Icon: Mail },
  { id: "chat", label: "Chat", Icon: MessageSquare },
  { id: "notes", label: "Docs and notes", Icon: NotebookPen },
  { id: "code", label: "Code and prompts", Icon: Code },
];

const USE_KEY = "lipik.onboarding.usecases";

const readUseCases = (): UseCase[] => {
  try {
    const raw = JSON.parse(localStorage.getItem(USE_KEY) ?? "[]");
    return Array.isArray(raw)
      ? raw.filter((x) => USE_CASES.some((u) => u.id === x))
      : [];
  } catch {
    return [];
  }
};

const Welcome = ({ next, back }: StepProps) => {
  const { key } = useHotkey();
  const [picked, setPicked] = useState<UseCase[]>(() =>
    isFlowPreview ? ["email", "chat"] : readUseCases(),
  );
  const toggle = (id: UseCase) =>
    setPicked((p) => (p.includes(id) ? p.filter((x) => x !== id) : [...p, id]));
  const save = () => {
    try {
      localStorage.setItem(USE_KEY, JSON.stringify(picked));
    } catch {
      /* the practice step falls back to email */
    }
    next();
  };
  return (
    <StepLayout
      eyebrow={`Welcome to ${APP_NAME}`}
      title={`Speak. ${APP_NAME} writes it down.`}
      body="Hold a key, say what you mean, let go. Clean text lands wherever you are typing."
      back={back}
      actions={<Continue next={save} disabled={picked.length === 0} />}
      visual={<WritingVisual pill="recording" hotkey={key} />}
    >
      <div className="fk-eyebrow mb-[8px]">Where will you use it most?</div>
      <div className="grid grid-cols-2 gap-[8px]">
        {USE_CASES.map(({ id, label, Icon }) => {
          const on = picked.includes(id);
          return (
            <button
              key={id}
              type="button"
              aria-pressed={on}
              onClick={() => toggle(id)}
              className={`flex h-[48px] items-center gap-[12px] rounded-[10px] px-[16px] text-left text-[15px] transition-colors duration-[var(--fk-t-quick)] ${on ? "bg-[var(--fk-accent-soft)] shadow-[inset_0_0_0_1px_var(--fk-accent)]" : "bg-[var(--fk-surface)] hover:bg-[var(--fk-hover)]"}`}
            >
              <Icon
                size={16}
                strokeWidth={1.75}
                className={`shrink-0 ${on ? "text-[var(--fk-accent)]" : "text-[var(--fk-text-muted)]"}`}
              />
              <span className="flex-1">{label}</span>
              <span
                className={`flex h-[18px] w-[18px] items-center justify-center rounded-[4px] ${on ? "bg-[var(--fk-accent)] text-[var(--fk-accent-text)]" : "shadow-[inset_0_0_0_1px_var(--fk-border-strong)]"}`}
              >
                {on && <Check size={12} strokeWidth={2.5} />}
              </span>
            </button>
          );
        })}
      </div>
    </StepLayout>
  );
};

/* ---------- permissions ---------- */

const PermissionRow = ({
  Icon,
  title,
  body,
  granted,
  onAllow,
}: {
  Icon: typeof Mic;
  title: string;
  body: string;
  granted: boolean | null;
  onAllow: () => void;
}) => (
  <div className="flex items-center gap-[16px] border-b border-[var(--fk-border)] px-[20px] py-[16px] last:border-b-0">
    <span className="flex h-[32px] w-[32px] shrink-0 items-center justify-center rounded-[8px] bg-[var(--fk-surface-2)] text-[var(--fk-text-muted)]">
      <Icon size={16} strokeWidth={1.75} />
    </span>
    <div className="min-w-0 flex-1">
      <div className="text-[15px] font-semibold leading-[22px]">{title}</div>
      <div className="text-[13px] leading-[18px] text-[var(--fk-text-muted)]">
        {body}
      </div>
    </div>
    {granted ? (
      <span className="flex h-[30px] shrink-0 items-center gap-[4px] rounded-[8px] bg-[var(--fk-ok-soft)] px-[10px] text-[13.5px] font-semibold text-[var(--fk-ok)]">
        <Check size={14} strokeWidth={2} />
        Allowed
      </span>
    ) : (
      <Button
        size="sm"
        onClick={onAllow}
        disabled={granted === null}
        className="shrink-0"
      >
        Allow
      </Button>
    )}
  </div>
);

const Permissions = ({ next, back }: StepProps) => {
  const { microphone, accessibility, request, timedOut, needsAccessibility } =
    usePermissions();
  const allGranted = !!microphone && (!needsAccessibility || !!accessibility);
  const ask = (kind: PermissionKind) => request(kind);
  return (
    <StepLayout
      eyebrow="Permissions"
      title={`Let ${APP_NAME} hear and type`}
      body="macOS asks you to confirm each one."
      back={back}
      actions={
        allGranted ? (
          <Continue next={next} />
        ) : (
          <>
            <LinkButton onClick={next}>Set up later</LinkButton>
            <Continue next={next} disabled />
          </>
        )
      }
      visual={
        <Card tone="panel" className="overflow-hidden">
          <PermissionRow
            Icon={Mic}
            title="Microphone"
            body={`So ${APP_NAME} can hear you while you hold the key.`}
            granted={microphone}
            onAllow={() => ask("microphone")}
          />
          {needsAccessibility && (
            <PermissionRow
              Icon={Keyboard}
              title="Accessibility"
              body={`So ${APP_NAME} can type the text into the app you're in.`}
              granted={accessibility}
              onAllow={() => ask("accessibility")}
            />
          )}
        </Card>
      }
    >
      {timedOut && (
        <p className="flex items-center gap-[8px] text-[14px] leading-[20px] text-[var(--fk-text-muted)]">
          <CircleAlert
            size={16}
            strokeWidth={1.75}
            className="shrink-0 text-[var(--fk-live)]"
          />
          macOS didn't answer. Try again, or restart {APP_NAME}.
        </p>
      )}
    </StepLayout>
  );
};

/* ---------- microphone test ---------- */

const MicTest = ({ next, back }: StepProps) => {
  const { getSetting, updateSetting, audioDevices, refreshAudioDevices } =
    useSettings();
  useEffect(() => {
    if (!isFlowPreview) refreshAudioDevices();
  }, [refreshAudioDevices]);
  const selected = getSetting("selected_microphone");
  const value = !selected || selected === "default" ? "Default" : selected;
  const level = useMicLevel(true, value === "Default" ? null : value);
  const [heard, setHeard] = useState(isFlowPreview);
  useEffect(() => {
    if (level !== null && level > 0.25) setHeard(true);
  }, [level]);
  const options = (
    audioDevices.length ? audioDevices : [{ name: "Default" }]
  ).map((d) => ({ value: d.name, label: d.name }));
  return (
    <StepLayout
      eyebrow="Microphone"
      title={`Let ${APP_NAME} hear you`}
      body={
        level === null
          ? "Say a few words. You can check your microphone again in the next step."
          : "Say a few words. The lines grow when you speak and settle when you stop."
      }
      back={back}
      actions={
        <>
          {heard && <OkNote>{APP_NAME} can hear you</OkNote>}
          <Continue next={next} />
        </>
      }
      visual={<MeterVisual level={level ?? 0} live={level !== null} />}
    >
      <div className="flex items-center justify-between gap-[16px]">
        <span className="text-[14px] font-medium text-[var(--fk-text-muted)]">
          Microphone
        </span>
        <Select
          label="Microphone"
          value={value}
          width={320}
          options={options}
          onChange={(v) => updateSetting("selected_microphone", v)}
        />
      </div>
    </StepLayout>
  );
};

/* ---------- shortcut ---------- */

const Shortcut = ({ next, back }: StepProps) => {
  const { binding, key } = useHotkey();
  const [edit, setEdit] = useState(false);
  return (
    <StepLayout
      eyebrow="Your key"
      title={
        <>
          Hold <Keys label={key} size="lg" /> to write
        </>
      }
      body={`Hold it, speak, then let go. ${APP_NAME} types where your cursor is. Tap it twice to keep going hands-free.`}
      back={back}
      actions={
        <>
          {!edit && (
            <LinkButton onClick={() => setEdit(true)}>Change key</LinkButton>
          )}
          <Continue next={next} />
        </>
      }
      visual={<KeyboardVisual binding={binding} hotkey={key} />}
    >
      {edit && (
        <div className="fk-set fk-shortcut">
          <ShortcutInput shortcutId="transcribe" grouped />
        </div>
      )}
    </StepLayout>
  );
};

/* ---------- clean-up model ---------- */

const CLEANUP_SKIP_NOTE = `${APP_NAME} still types what you say and fixes “um”, spoken punctuation, numbers and emails without it. You can set this up later in Settings.`;

const Cleanup = ({ next, back }: StepProps) => {
  const { view, model, progress, error, download, cancel } = useCleanupModel();
  const ready = view === "ready";
  return (
    <StepLayout
      eyebrow="Clean-up"
      title="Clean text, not a transcript"
      body="Clean-up removes “um”, applies “no, wait” corrections and fixes punctuation, on this Mac."
      back={back}
      actions={
        ready ? (
          <Continue next={next} />
        ) : (
          <>
            <LinkButton onClick={next}>Skip for now</LinkButton>
            <Continue next={next} disabled />
          </>
        )
      }
      visual={<CleanupVisual />}
    >
      {view === "checking" && (
        <p className="flex items-center gap-[8px] text-[14px] leading-[20px] text-[var(--fk-text-muted)]">
          <Loader2 size={16} strokeWidth={1.75} className="animate-spin" />
          Checking for Ollama…
        </p>
      )}
      {ready && <OkNote>Clean-up is ready. It uses {model} in Ollama.</OkNote>}
      {view === "no_ollama" && (
        <div className="flex items-center gap-[16px]">
          <p className="flex-1 text-[14px] leading-[20px] text-[var(--fk-text-muted)]">
            It needs Ollama, a free app that runs the model. If you have it
            already, open it. This step moves on by itself once Ollama runs.
          </p>
          <Button onClick={openOllamaDownload} className="shrink-0">
            Get Ollama (free)
          </Button>
        </div>
      )}
      {view === "no_model" && (
        <div className="flex items-center gap-[16px]">
          <p className="flex-1 text-[14px] leading-[20px] text-[var(--fk-text-muted)]">
            Ollama is running. Now get the model it uses for clean-up.
          </p>
          <Button onClick={download} className="shrink-0">
            {downloadLabel(model)}
          </Button>
        </div>
      )}
      {view === "slow_model" && (
        <p className="flex items-start gap-[8px] text-[14px] leading-[20px] text-[var(--fk-text-muted)]">
          <CircleAlert
            size={16}
            strokeWidth={1.75}
            className="mt-[2px] shrink-0 text-[var(--fk-live)]"
          />
          <span>{SLOW_MODEL_TEXT}</span>
        </p>
      )}
      {view === "downloading" && (
        <PullProgress progress={progress} onCancel={cancel} />
      )}
      {view === "failed" && (
        <div className="flex items-center gap-[16px]">
          <p className="flex flex-1 items-start gap-[8px] text-[14px] leading-[20px] text-[var(--fk-text-muted)]">
            <CircleAlert
              size={16}
              strokeWidth={1.75}
              className="mt-[2px] shrink-0 text-[var(--fk-live)]"
            />
            <span>The download didn't finish. {error}</span>
          </p>
          <Button onClick={download} className="shrink-0">
            Try again
          </Button>
        </div>
      )}
      {!ready && view !== "checking" && (
        <p className="mt-[16px] text-[13px] leading-[18px] text-[var(--fk-text-faint)]">
          {CLEANUP_SKIP_NOTE}
        </p>
      )}
    </StepLayout>
  );
};

/* ---------- practice ---------- */

const PROMPTS: Record<
  UseCase,
  { app: string; Icon: typeof Mail; to: string; say: string }
> = {
  email: {
    app: "Mail",
    Icon: Mail,
    to: "To: Sam",
    say: "Hi Sam, thanks for sending the notes. I'll read them tonight and reply by tomorrow morning.",
  },
  chat: {
    app: "Messages",
    Icon: MessageSquare,
    to: "#team",
    say: "Running five minutes late, start without me and I'll catch up.",
  },
  notes: {
    app: "Notes",
    Icon: NotebookPen,
    to: "Ideas",
    say: "Things to try this week: shorter standups, a shared doc for questions, and one quiet afternoon.",
  },
  code: {
    app: "Editor",
    Icon: Code,
    to: "Prompt",
    say: "Write a function that removes duplicate emails from a list and keeps the first one it sees.",
  },
};

const ModelLine = () => {
  const setup = useModelSetup();
  if (setup.status === "ready") return null;
  const text =
    setup.status === "failed"
      ? "The speech model didn't download."
      : setup.status === "downloading"
        ? `Downloading the speech model… ${setup.percent}%`
        : setup.status === "verifying"
          ? "Checking the speech model…"
          : setup.status === "loading"
            ? "Loading the speech model…"
            : "Preparing the speech model…";
  return (
    <div className="mt-[16px] flex items-center gap-[8px] text-[14px] leading-[20px] text-[var(--fk-text-muted)]">
      {setup.status === "failed" ? (
        <CircleAlert
          size={16}
          strokeWidth={1.75}
          className="text-[var(--fk-live)]"
        />
      ) : (
        <Loader2 size={16} strokeWidth={1.75} className="animate-spin" />
      )}
      <span className="fk-num">{text}</span>
      {setup.status === "failed" && (
        <Button variant="accent" size="sm" onClick={retryDefaultModelSetup}>
          Try again
        </Button>
      )}
    </div>
  );
};

const Practice = ({ next, back }: StepProps) => {
  const { key } = useHotkey();
  const [cases] = useState(() => {
    const picked = isFlowPreview
      ? (["email", "chat"] as UseCase[])
      : readUseCases();
    return picked.length ? picked : (["email"] as UseCase[]);
  });
  const [tab, setTab] = useState(0);
  const [text, setText] = useState("");
  const [done, setDone] = useState(false);
  const area = useRef<HTMLTextAreaElement>(null);
  const prompt = PROMPTS[cases[tab]];

  useEffect(() => {
    area.current?.focus();
  }, [tab]);

  useEffect(() => {
    if (isFlowPreview) return;
    const unlisten = events.historyUpdatePayload.listen(({ payload }) => {
      if (payload.action !== "added") return;
      const said = entryText(payload.entry);
      if (!said) return;
      setDone(true);
      setTimeout(() => {
        const el = area.current;
        if (el && !el.value.includes(said)) {
          setText((t) => (t ? `${t} ${said}` : said));
        }
      }, 600);
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  return (
    <StepLayout
      eyebrow="Practice"
      title="Try it once"
      body={
        <>
          Click in the box, hold <Keys label={key} />, read the line out loud,
          then let go.
        </>
      }
      back={back}
      lead={
        <div className="rounded-[14px] border-l-[3px] border-[var(--fk-accent)] bg-[var(--fk-accent-soft)] px-[20px] py-[12px]">
          <div className="fk-eyebrow mb-[4px]">Read this</div>
          <p className="fk-display text-[17px] leading-[28px] text-[var(--fk-text)]">
            “{prompt.say.replace(/'/g, "’")}”
          </p>
        </div>
      }
      actions={
        <>
          {done ? (
            <OkNote>{APP_NAME} wrote that</OkNote>
          ) : (
            <Button variant="ghost" onClick={next}>
              Skip
            </Button>
          )}
          <Continue next={next} disabled={!done} />
        </>
      }
      visual={
        <Card tone="panel" className="flex flex-col overflow-hidden">
          <div className="flex h-[44px] items-center gap-[4px] border-b border-[var(--fk-border)] px-[12px]">
            {cases.map((c, i) => {
              const { app, Icon } = PROMPTS[c];
              return (
                <button
                  key={c}
                  type="button"
                  onClick={() => setTab(i)}
                  className={`flex h-[28px] items-center gap-[6px] rounded-[8px] px-[8px] text-[13px] font-medium transition-colors duration-[var(--fk-t-quick)] ${i === tab ? "bg-[var(--fk-active)] text-[var(--fk-accent)]" : "text-[var(--fk-text-muted)] hover:bg-[var(--fk-hover)]"}`}
                >
                  <Icon size={14} strokeWidth={1.75} />
                  {app}
                </button>
              );
            })}
            <span className="ml-auto truncate text-[13px] text-[var(--fk-text-faint)]">
              {prompt.to}
            </span>
          </div>
          <textarea
            ref={area}
            value={text}
            onChange={(e) => setText(e.target.value)}
            placeholder={`Hold ${key} and speak…`}
            className="h-[112px] resize-none bg-transparent !shadow-none px-[20px] py-[12px] text-[15px] leading-[24px] text-[var(--fk-text)] outline-none placeholder:text-[var(--fk-text-faint)]"
          />
          <div className="flex h-[32px] items-center justify-center">
            <PillPreview state={done ? "done" : "idle"} />
          </div>
        </Card>
      }
    >
      <ModelLine />
    </StepLayout>
  );
};

/* ---------- the pill ---------- */

const PillStep = ({ next, back }: StepProps) => {
  const { key } = useHotkey();
  return (
    <StepLayout
      eyebrow="The pill"
      title="Always at the bottom"
      body={
        <>
          The pill shows when {APP_NAME} is listening. Hold <Keys label={key} />{" "}
          or click it to start, and drag it along the edge if it is in the way.
        </>
      }
      back={back}
      actions={<Continue next={next} />}
      visual={<WritingVisual pill="hover" hotkey={key} />}
    />
  );
};

/* ---------- AI apps ---------- */

const AiApps = ({ next, back }: StepProps) => {
  const state = useAiApps();
  const found = state.apps?.filter((a) => a.installed) ?? null;
  return (
    <StepLayout
      eyebrow="AI apps"
      title="Ask your AI about your meetings"
      body={`Claude, Cursor, Codex and other AI apps can read your ${APP_NAME} meeting notes on this Mac. They can't see your dictations.`}
      back={back}
      actions={<Continue next={next} />}
      visual={<AiAppsVisual />}
    >
      {found && found.length > 0 && (
        <AiAppRows state={{ ...state, apps: found }} />
      )}
      {found && found.length === 0 && (
        <p className="text-[14px] leading-[20px] text-[var(--fk-text-muted)]">
          No AI apps found on this Mac. You can connect one later in Settings,
          AI apps.
        </p>
      )}
    </StepLayout>
  );
};

/* ---------- privacy ---------- */

const Privacy = ({ next, back }: StepProps) => (
  <StepLayout
    eyebrow="Privacy"
    title="Your words stay here"
    body={`${APP_NAME} does its work on this Mac.`}
    back={back}
    actions={<Continue next={next} />}
    visual={
      <Card tone="panel" className="overflow-hidden">
        {[
          "Speech is turned into text on this Mac. Your audio is never uploaded.",
          `Recordings and their text stay in ${APP_NAME}'s data folder so it can learn your voice. You can change this in Settings.`,
          "Clean-up runs on a model on this Mac through the free Ollama app, not in the cloud.",
        ].map((line) => (
          <div
            key={line}
            className="flex gap-[12px] border-b border-[var(--fk-border)] px-[20px] py-[16px] text-[15px] leading-[22px] last:border-b-0"
          >
            <ShieldCheck
              size={16}
              strokeWidth={1.75}
              className="mt-[3px] shrink-0 text-[var(--fk-accent)]"
            />
            <span>{line}</span>
          </div>
        ))}
      </Card>
    }
  />
);

/* ---------- done ---------- */

const Done = ({ next, back }: StepProps) => {
  const { key } = useHotkey();
  return (
    <StepLayout
      eyebrow="Ready"
      title="You’re set"
      body={
        <>
          Hold <Keys label={key} /> in any app, or tap it twice to keep going
          hands-free.
        </>
      }
      back={back}
      actions={<Continue next={next} label={`Open ${APP_NAME}`} />}
      visual={<MarkVisual />}
    >
      <ModelLine />
    </StepLayout>
  );
};

export type StepId =
  | "welcome"
  | "permissions"
  | "mic"
  | "shortcut"
  | "cleanup"
  | "practice"
  | "bar"
  | "ai"
  | "privacy"
  | "done";

export const STEPS: {
  id: StepId;
  Component: ComponentType<StepProps>;
  skippable?: boolean;
}[] = [
  { id: "welcome", Component: Welcome },
  { id: "permissions", Component: Permissions },
  { id: "mic", Component: MicTest, skippable: true },
  { id: "shortcut", Component: Shortcut },
  { id: "cleanup", Component: Cleanup },
  { id: "practice", Component: Practice },
  { id: "bar", Component: PillStep, skippable: true },
  { id: "ai", Component: AiApps, skippable: true },
  { id: "privacy", Component: Privacy },
  { id: "done", Component: Done },
];
