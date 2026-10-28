import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ArrowUpCircle, X } from "lucide-react";
import type { Update } from "@tauri-apps/plugin-updater";
import { commands, events } from "@/bindings";
import {
  findUpdate,
  installUpdate,
  openDownloadPage,
  restartApp,
} from "@/lib/selfUpdate";
import { APP_NAME } from "./brand";
import { Button, IconButton } from "./ui";

const CHECK_EVERY_MS = 3 * 60 * 60 * 1000;
const NOTES_URL = "https://github.com/Gourav2609/lipik/releases/tag/v";
const SKIPPED_KEY = "lipik.update.skipped";

const skipped = () => {
  try {
    return localStorage.getItem(SKIPPED_KEY);
  } catch {
    return null;
  }
};

type Stage =
  | { kind: "offer" }
  | { kind: "downloading"; percent: number }
  | { kind: "restarting" }
  | { kind: "failed" }
  | { kind: "restartLater" };

/** A new version on the update feed, every few hours; "Later" hides that version. */
export const UpdateNotice = () => {
  const [release, setRelease] = useState<Update | null>(null);
  const [stage, setStage] = useState<Stage>({ kind: "offer" });
  const [meeting, setMeeting] = useState(false);

  useEffect(() => {
    const check = async () => {
      const r = await commands.getAppSettings().catch(() => null);
      if (r?.status !== "ok" || !(r.data.release_check_enabled ?? true)) return;
      const u = await findUpdate();
      setRelease((cur) =>
        cur && cur.version === u?.version
          ? cur
          : u && u.version !== skipped()
            ? u
            : null,
      );
    };
    check();
    const id = window.setInterval(check, CHECK_EVERY_MS);
    return () => window.clearInterval(id);
  }, []);

  useEffect(() => {
    commands
      .notetakerStatus()
      .then((r) => r.status === "ok" && setMeeting(r.data.recording))
      .catch(() => {});
    const off = events.notetakerState.listen((e) =>
      setMeeting(e.payload.recording),
    );
    return () => {
      off.then((fn) => fn());
    };
  }, []);

  if (!release) return null;

  const later = () => {
    try {
      localStorage.setItem(SKIPPED_KEY, release.version);
    } catch {
      /* the notice just comes back next time */
    }
    setRelease(null);
  };

  const restart = () => {
    setStage({ kind: "restarting" });
    restartApp().catch(() => setStage({ kind: "restartLater" }));
  };

  const install = async () => {
    setStage({ kind: "downloading", percent: 0 });
    try {
      await installUpdate(release, (percent) =>
        setStage({ kind: "downloading", percent }),
      );
    } catch {
      setStage({ kind: "failed" });
      return;
    }
    const status = await commands.notetakerStatus().catch(() => null);
    if (status?.status === "ok" && status.data.recording)
      setStage({ kind: "restartLater" });
    else restart();
  };

  const note =
    stage.kind === "downloading"
      ? `Downloading… ${stage.percent}%`
      : stage.kind === "restarting"
        ? "Restarting…"
        : stage.kind === "failed"
          ? "Couldn't update. Try again later."
          : stage.kind === "restartLater"
            ? meeting
              ? "Restart after your meeting to finish updating."
              : `Restart ${APP_NAME} to finish updating.`
            : "Your settings and history stay.";

  return (
    <div className="fk-notice mx-auto w-full max-w-[1080px] px-[48px] pt-[16px]">
      <div className="flex items-center gap-[12px] rounded-[14px] bg-[var(--fk-surface)] px-[20px] py-[14px]">
        <ArrowUpCircle
          size={18}
          strokeWidth={1.75}
          className="shrink-0 text-[var(--fk-accent)]"
        />
        <p className="min-w-0 flex-1 text-[14.5px] leading-[22px]">
          <span className="font-semibold">
            {APP_NAME} {release.version} is out.
          </span>{" "}
          <span className="fk-num text-[var(--fk-text-muted)]">{note}</span>
        </p>
        <Button
          variant="ghost"
          size="sm"
          className="!text-[var(--fk-text-muted)]"
          onClick={() => openUrl(NOTES_URL + release.version).catch(() => {})}
        >
          What's new
        </Button>
        {stage.kind === "offer" && (
          <Button size="sm" onClick={install}>
            Update now
          </Button>
        )}
        {stage.kind === "failed" && (
          <Button size="sm" onClick={openDownloadPage}>
            Open website
          </Button>
        )}
        {stage.kind === "restartLater" && !meeting && (
          <Button size="sm" onClick={restart}>
            Restart
          </Button>
        )}
        {(stage.kind === "offer" ||
          stage.kind === "failed" ||
          (stage.kind === "restartLater" && meeting)) && (
          <IconButton label="Later" className="-mr-[6px]" onClick={later}>
            <X size={16} strokeWidth={1.75} />
          </IconButton>
        )}
      </div>
    </div>
  );
};
