import { useCallback, useEffect, useState } from "react";
import { Check, Copy } from "lucide-react";
import { commands, type AiApp } from "@/bindings";
import { APP_NAME } from "./brand";
import { AppTile } from "./settings/apps";
import { SettingsCard } from "./settings/parts";
import { Button } from "./ui";

/** AI apps that can read meeting notes through `lipik --mcp`. */
export const useAiApps = () => {
  const [apps, setApps] = useState<AiApp[] | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [errors, setErrors] = useState<Record<string, string>>({});

  const refresh = useCallback(() => {
    commands
      .aiAppsStatus()
      .then(setApps)
      .catch(() => setApps([]));
  }, []);
  useEffect(refresh, [refresh]);

  const change = async (app: AiApp, connect: boolean) => {
    setBusy(app.id);
    try {
      const res = connect
        ? await commands.connectAiApp(app.id)
        : await commands.disconnectAiApp(app.id);
      if (res.status === "ok") {
        setErrors(({ [app.id]: _, ...rest }) => rest);
        setApps(
          (list) => list?.map((a) => (a.id === app.id ? res.data : a)) ?? null,
        );
      } else {
        setErrors((e) => ({ ...e, [app.id]: res.error }));
      }
    } catch (e) {
      setErrors((err) => ({ ...err, [app.id]: String(e) }));
    } finally {
      setBusy(null);
    }
  };

  const sorted =
    apps && [...apps].sort((a, b) => Number(b.installed) - Number(a.installed));
  return { apps: sorted, busy, errors, change, refresh };
};

const CopyButton = ({ text, label }: { text: string; label: string }) => {
  const [done, setDone] = useState(false);
  return (
    <Button
      variant="secondary"
      size="sm"
      icon={
        done ? (
          <Check size={14} strokeWidth={1.75} />
        ) : (
          <Copy size={14} strokeWidth={1.75} />
        )
      }
      onClick={() =>
        navigator.clipboard.writeText(text).then(() => {
          setDone(true);
          window.setTimeout(() => setDone(false), 1500);
        })
      }
    >
      {done ? "Copied" : label}
    </Button>
  );
};

const describe = (app: AiApp) => {
  if (!app.installed) return "Not on this Mac.";
  if (app.how === "copy") return app.note ?? "";
  if (app.connected)
    return app.note ?? `Can read your ${APP_NAME} meeting notes.`;
  if (app.how === "command") return `Runs “claude mcp add” to add ${APP_NAME}.`;
  return app.file
    ? `Adds ${APP_NAME} to ${app.file.replace(/^\/Users\/[^/]+/, "~")}`
    : "";
};

/** One row per AI app: connect, remove, or copy the setup by hand. */
export const AiAppRows = ({
  state,
}: {
  state: ReturnType<typeof useAiApps>;
}) => {
  const { apps, busy, errors, change } = state;
  if (!apps) {
    return (
      <SettingsCard>
        <div className="py-[20px] text-[14px] text-[var(--fk-text-muted)]">
          Looking for AI apps…
        </div>
      </SettingsCard>
    );
  }
  return (
    <SettingsCard>
      {apps.map((app) => (
        <div
          key={app.id}
          className={`flex min-h-[64px] items-center gap-[12px] border-b border-[var(--fk-border)] py-[12px] last:border-b-0 ${app.installed ? "" : "opacity-60"}`}
        >
          <AppTile name={app.name} />
          <div className="min-w-0 flex-1">
            <div className="text-[15px] font-medium leading-[22px]">
              {app.name}
            </div>
            <div className="mt-[2px] text-[13px] leading-[18px] text-[var(--fk-text-muted)]">
              {describe(app)}
            </div>
            {errors[app.id] && (
              <div className="mt-[4px] text-[13px] leading-[18px] text-[var(--fk-live)]">
                {errors[app.id]}
              </div>
            )}
          </div>
          <div className="flex shrink-0 items-center gap-[8px]">
            {app.installed && app.how === "copy" && app.connected && (
              <span className="text-[13px] font-medium text-[var(--fk-ok)]">
                Connected
              </span>
            )}
            {app.installed && app.how !== "copy" && app.connected && (
              <>
                <span className="text-[13px] font-medium text-[var(--fk-ok)]">
                  Connected
                </span>
                <Button
                  variant="ghost"
                  size="sm"
                  disabled={busy === app.id}
                  onClick={() => change(app, false)}
                >
                  Remove
                </Button>
              </>
            )}
            {app.installed && app.how !== "copy" && !app.connected && (
              <Button
                variant="secondary"
                size="sm"
                disabled={busy === app.id}
                onClick={() => change(app, true)}
              >
                {busy === app.id ? "Adding…" : "Connect"}
              </Button>
            )}
            {(errors[app.id] || (app.installed && app.how === "copy")) &&
              app.manual && <CopyButton text={app.manual} label="Copy setup" />}
          </div>
        </div>
      ))}
    </SettingsCard>
  );
};

/** The command any other MCP app needs. */
export const otherAppsSetup = (apps: AiApp[] | null) => {
  const fromFile = apps?.find(
    (a) => a.how !== "copy" && a.manual.includes("--mcp"),
  );
  const command =
    fromFile?.manual.match(/"([^"]*\/lipik)"/)?.[1] ??
    "/Applications/Lipik.app/Contents/MacOS/lipik";
  return `Command: ${command}\nArguments: --mcp`;
};

export const OtherApps = ({ apps }: { apps: AiApp[] | null }) => (
  <div className="flex items-center gap-[16px]">
    <p className="flex-1 text-[14px] leading-[20px] text-[var(--fk-text-muted)]">
      Any other app that supports MCP can run {APP_NAME} with this command and
      the argument --mcp.
    </p>
    <CopyButton text={otherAppsSetup(apps)} label="Copy command" />
  </div>
);
