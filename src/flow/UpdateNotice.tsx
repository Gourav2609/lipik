import { useEffect, useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ArrowUpCircle, X } from "lucide-react";
import { commands, type NewRelease } from "@/bindings";
import { APP_NAME } from "./brand";
import { Button, IconButton } from "./ui";

const CHECK_EVERY_MS = 24 * 60 * 60 * 1000;
const SKIPPED_KEY = "lipik.update.skipped";

const skipped = () => {
  try {
    return localStorage.getItem(SKIPPED_KEY);
  } catch {
    return null;
  }
};

/** A newer release on GitHub, once a day; "Later" hides that version. */
export const UpdateNotice = () => {
  const [release, setRelease] = useState<NewRelease | null>(null);

  useEffect(() => {
    const check = () =>
      commands
        .checkForNewRelease()
        .then((r) => setRelease(r && r.version !== skipped() ? r : null))
        .catch(() => {});
    check();
    const id = window.setInterval(check, CHECK_EVERY_MS);
    return () => window.clearInterval(id);
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
          <span className="text-[var(--fk-text-muted)]">
            Download it and replace the app in Applications. Your settings and
            history stay.
          </span>
        </p>
        <Button size="sm" onClick={() => openUrl(release.url).catch(() => {})}>
          See what's new
        </Button>
        <IconButton label="Later" className="-mr-[6px]" onClick={later}>
          <X size={16} strokeWidth={1.75} />
        </IconButton>
      </div>
    </div>
  );
};
