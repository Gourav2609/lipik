import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useSettings } from "@/hooks/useSettings";
import { Button } from "../ui";
import { Select, SettingRow } from "./parts";

type Engine = "parakeet" | "apple" | "auto";

interface AppleSpeechStatus {
  supported: boolean;
  locale: string | null;
  ready: boolean;
  code: string | null;
  error: string | null;
}

const ENGINE_OPTIONS = [
  { value: "auto", label: "Auto (Hinglish mode)" },
  { value: "parakeet", label: "Local model only" },
  { value: "apple", label: "Apple dictation" },
];

const ENGINE_TEXT: Record<Engine, string> = {
  auto: "Uses the local model for English and switches to Apple dictation when you mix in Hindi.",
  parakeet: "Uses only the model above.",
  apple: "Uses Apple's on-device dictation. The model above takes over if it fails.",
};

const LOCALE_OPTIONS = [
  { value: "en-IN", label: "English (India)" },
  { value: "en-US", label: "English (US)" },
  { value: "en-GB", label: "English (UK)" },
];

const statusText = (status: AppleSpeechStatus | null, busy: boolean) => {
  if (busy) return "Downloading Apple's speech files…";
  if (!status) return "Apple dictation listens in this language.";
  if (status.ready) return "Ready. Works offline.";
  if (status.code === "assets_missing")
    return "Apple's speech files for this language aren't on this Mac yet.";
  if (status.code === "unsupported_locale")
    return "Apple dictation doesn't support this language.";
  return "Apple dictation isn't working right now, so the local model is used.";
};

/** Speech engine choice and, when Apple's dictation is in use, its language. */
export const SpeechEngineRows = () => {
  const { settings, refreshSettings } = useSettings();
  const saved = settings as unknown as {
    speech_engine?: Engine;
    apple_locale?: string;
  } | null;
  const [engine, setEngine] = useState<Engine | null>(null);
  const [locale, setLocale] = useState<string | null>(null);
  const [status, setStatus] = useState<AppleSpeechStatus | null>(null);
  const [busy, setBusy] = useState(false);

  const currentEngine = engine ?? saved?.speech_engine ?? "auto";
  const currentLocale = locale ?? saved?.apple_locale ?? "en-IN";
  const usesApple = currentEngine !== "parakeet";

  useEffect(() => {
    if (!usesApple) return;
    let live = true;
    invoke<AppleSpeechStatus | null>("get_apple_speech_status", {
      download: false,
    })
      .then((s) => live && setStatus(s ?? null))
      .catch(() => live && setStatus(null));
    return () => {
      live = false;
    };
  }, [usesApple, currentLocale]);

  const chooseEngine = (next: string) => {
    const prev = currentEngine;
    setEngine(next as Engine);
    invoke("change_speech_engine_setting", { engine: next })
      .then(refreshSettings)
      .catch(() => setEngine(prev));
  };

  const chooseLocale = (next: string) => {
    const prev = currentLocale;
    setLocale(next);
    invoke("change_apple_locale_setting", { locale: next })
      .then(refreshSettings)
      .catch(() => setLocale(prev));
  };

  const download = () => {
    setBusy(true);
    invoke<AppleSpeechStatus | null>("get_apple_speech_status", {
      download: true,
    })
      .then((s) => setStatus(s ?? null))
      .catch(() => {})
      .finally(() => setBusy(false));
  };

  const unsupported = status !== null && !status.supported;
  const localeOptions = LOCALE_OPTIONS.some((o) => o.value === currentLocale)
    ? LOCALE_OPTIONS
    : [...LOCALE_OPTIONS, { value: currentLocale, label: currentLocale }];

  return (
    <>
      <SettingRow
        title="Speech engine"
        description={
          usesApple && unsupported
            ? "Apple dictation needs macOS 26, so the local model is used."
            : ENGINE_TEXT[currentEngine]
        }
        control={
          <Select
            label="Speech engine"
            value={currentEngine}
            options={ENGINE_OPTIONS}
            disabled={busy}
            onChange={chooseEngine}
          />
        }
      />
      {usesApple && !unsupported && (
        <SettingRow
          title="Apple dictation language"
          description={statusText(status, busy)}
          control={
            <div className="flex items-center gap-[8px]">
              {status?.code === "assets_missing" && (
                <Button variant="secondary" disabled={busy} onClick={download}>
                  Download
                </Button>
              )}
              <Select
                label="Apple dictation language"
                value={currentLocale}
                options={localeOptions}
                disabled={busy}
                onChange={chooseLocale}
              />
            </div>
          }
        />
      )}
    </>
  );
};
