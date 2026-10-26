import { useCallback, useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { listen } from "@tauri-apps/api/event";
import { KeyboardOff, X } from "lucide-react";
import { commands, type SecureInputStatus } from "@/bindings";
import { useOsType } from "@/hooks/useOsType";
import { useSettings } from "@/hooks/useSettings";
import { formatKeyCombination } from "@/lib/utils/keyboard";
import { IconButton, Kbd } from "./ui";

// Processes that hold Secure Input under a name people would not recognise.
const PLAIN_NAMES: Record<string, string> = {
  SecurityAgent: "a macOS password prompt",
};
const TERMINALS = ["Terminal", "iTerm2", "iTerm"];

const Keys = ({ binding }: { binding: string }) => (
  <span className="inline-flex gap-[4px] align-baseline">
    {formatKeyCombination(binding, useOsType())
      .split(" + ")
      .map((k) => (
        <Kbd key={k}>{k}</Kbd>
      ))}
  </span>
);

/**
 * Shown while macOS Secure Input blocks a shortcut. Secure Input hides key
 * presses from every app (it protects password fields); modifier-only
 * shortcuts such as holding fn keep working, so the notice names exactly
 * what is paused and how to clear it.
 */
export const SecureInputNotice = () => {
  const { t } = useTranslation();
  const bindings = useSettings().settings?.bindings;
  const [status, setStatus] = useState<SecureInputStatus | null>(null);
  const [dismissed, setDismissed] = useState(false);

  const refresh = useCallback(() => {
    commands
      .getSecureInputStatus()
      .then(setStatus)
      .catch(() => {});
  }, []);

  useEffect(() => {
    refresh();
    const unlisten = listen<SecureInputStatus>("secure-input-changed", (e) =>
      setStatus(e.payload),
    );
    return () => {
      unlisten.then((f) => f());
    };
  }, [refresh]);

  const paused = status
    ? [...new Set([...status.uncovered_bindings, ...status.degraded_bindings])]
    : [];
  const impacted =
    status !== null &&
    ((status.sustained && paused.length > 0) || status.recorder_blocked);

  useEffect(() => {
    if (!impacted) setDismissed(false);
  }, [impacted]);

  if (!impacted || dismissed || !status) return null;

  const culprit = status.culprit_name;
  const terminal = culprit !== null && TERMINALS.includes(culprit);
  const [why, fix] =
    culprit === "loginwindow"
      ? [
          t(
            "secureInputNotice.whyLockScreen",
            "macOS is still blocking key presses from the lock screen.",
          ),
          t(
            "secureInputNotice.fixLockScreen",
            "Lock your Mac and unlock it again (Control + Command + Q) to clear it.",
          ),
        ]
      : terminal
        ? [
            t(
              "secureInputNotice.whyTerminal",
              "Secure Keyboard Entry in {{app}} hides key presses from other apps.",
              { app: culprit },
            ),
            t(
              "secureInputNotice.fixTerminal",
              "Turn it off in the {{app}} menu.",
              { app: culprit },
            ),
          ]
        : [
            t(
              "secureInputNotice.whyOther",
              "macOS hides key presses while {{holder}} is open, to keep passwords safe.",
              {
                holder: culprit
                  ? (PLAIN_NAMES[culprit] ?? culprit)
                  : t("secureInputNotice.passwordField", "a password field"),
              },
            ),
            t("secureInputNotice.fixOther", "It clears when you close it."),
          ];
  const dictateStillWorks = !paused.includes("transcribe");
  const pausedBinding =
    paused.length === 1 ? bindings?.[paused[0]]?.current_binding : undefined;

  return (
    <div className="fk-notice mx-auto w-full max-w-[1080px] px-[48px] pt-[16px]">
      <div className="flex items-start gap-[12px] rounded-[14px] bg-[var(--fk-surface)] px-[20px] py-[16px]">
        <KeyboardOff
          size={18}
          strokeWidth={1.75}
          className="mt-[2px] shrink-0 text-[var(--fk-text-muted)]"
        />
        <div className="min-w-0 flex-1">
          <p className="text-[14.5px] font-semibold leading-[22px]">
            {status.recorder_blocked && paused.length === 0 ? (
              t(
                "secureInputNotice.recorderTitle",
                "Shortcuts can't be changed right now",
              )
            ) : pausedBinding ? (
              <>
                <Keys binding={pausedBinding} />{" "}
                {t("secureInputNotice.titleOne", "is paused")}
              </>
            ) : (
              t("secureInputNotice.titleMany", "Some shortcuts are paused")
            )}
          </p>
          <p className="mt-[2px] text-[13.5px] leading-[20px] text-[var(--fk-text-muted)]">
            {why}{" "}
            {dictateStillWorks &&
              t(
                "secureInputNotice.fnWorks",
                "Holding fn to dictate still works.",
              )}{" "}
            {fix}
          </p>
        </div>
        <IconButton
          label={t("secureInput.dismiss", "Dismiss")}
          className="-mr-[6px] -mt-[4px]"
          onClick={() => setDismissed(true)}
        >
          <X size={16} strokeWidth={1.75} />
        </IconButton>
      </div>
    </div>
  );
};
