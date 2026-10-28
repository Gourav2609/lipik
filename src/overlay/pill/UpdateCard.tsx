import React, { useState } from "react";
import { ArrowUpCircle } from "lucide-react";
import type { Update } from "@tauri-apps/plugin-updater";
import { APP_NAME } from "@/flow/brand";
import { LipikMark } from "@/flow/ui/LipikMark";
import { installUpdate, openDownloadPage, restartApp } from "@/lib/selfUpdate";
import { XGlyph } from "./parts";
import { notetakerStatus } from "./backend";

// "Lipik 0.1.6 is out": the meeting card's shape, with Update now / Later.
// The new version only runs after a restart, which is not offered while a
// meeting is being recorded (restarting would end it).

export type UpdateOffer = { kind: "update"; key: number; version: string };

type Stage =
  | { kind: "offer" }
  | { kind: "downloading"; percent: number }
  | { kind: "restarting" }
  | { kind: "failed" }
  | { kind: "restartLater" };

type Props = {
  card: UpdateOffer;
  update: Update | null;
  style: React.CSSProperties;
  meeting: boolean;
  onBusy: (busy: boolean) => void;
  onLater: () => void;
};

export const UpdateCard: React.FC<Props> = ({
  card,
  update,
  style,
  meeting,
  onBusy,
  onLater,
}) => {
  const [stage, setStage] = useState<Stage>({ kind: "offer" });

  const restart = () => {
    setStage({ kind: "restarting" });
    restartApp().catch(() => setStage({ kind: "restartLater" }));
  };

  const install = async () => {
    onBusy(true);
    setStage({ kind: "downloading", percent: 0 });
    try {
      await installUpdate(update, (percent) =>
        setStage({ kind: "downloading", percent }),
      );
    } catch {
      onBusy(false);
      setStage({ kind: "failed" });
      return;
    }
    if ((await notetakerStatus())?.recording)
      setStage({ kind: "restartLater" });
    else restart();
  };

  const status =
    stage.kind === "downloading"
      ? `Downloading… ${stage.percent}%`
      : stage.kind === "restarting"
        ? "Restarting…"
        : stage.kind === "failed"
          ? "Couldn't update. Try again later."
          : stage.kind === "restartLater"
            ? meeting
              ? "Restart after your meeting to finish updating"
              : `Restart ${APP_NAME} to finish updating`
            : "Settings and history stay";

  const action = (label: string, onClick: () => void) => (
    <button className="card-main" onClick={onClick}>
      <span className="card-logo">
        <LipikMark size={12} />
      </span>
      <b>{label}</b>
    </button>
  );

  return (
    <div
      className={`note-card update ${stage.kind} ${meeting ? "meeting" : ""}`}
      style={style}
    >
      <ArrowUpCircle className="card-icon" size={15} strokeWidth={2} />
      <div className="card-text">
        <span className="card-title">
          {APP_NAME} {card.version} is out
        </span>
        <span className="card-status">
          {stage.kind !== "failed" && <i />}
          {status}
        </span>
      </div>
      {stage.kind === "offer" && (
        <div className="card-act">
          {action("Update now", install)}
          <button className="card-later" onClick={onLater}>
            Later
          </button>
        </div>
      )}
      {stage.kind === "failed" && (
        <div className="card-act">
          {action("Open website", openDownloadPage)}
        </div>
      )}
      {stage.kind === "restartLater" && !meeting && (
        <div className="card-act">{action("Restart", restart)}</div>
      )}
      {(stage.kind === "offer" ||
        stage.kind === "failed" ||
        (stage.kind === "restartLater" && meeting)) && (
        <button className="card-close" onClick={onLater}>
          <XGlyph size={8} stroke={1.3} />
        </button>
      )}
    </div>
  );
};
