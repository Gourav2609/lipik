import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";
import { openUrl } from "@tauri-apps/plugin-opener";

const DOWNLOAD_PAGE = "https://lipik.pages.dev/#download";

/** The newer version on the update feed, or null (also when the check fails). */
export const findUpdate = (): Promise<Update | null> =>
  check().catch(() => null);

/**
 * Downloads and installs the update, reporting 0-100. Throws when there is
 * nothing to install or the download fails. The new version runs after
 * `restartApp`.
 */
export const installUpdate = async (
  update: Update | null,
  onProgress: (percent: number) => void,
) => {
  const u = update ?? (await check());
  if (!u) throw new Error("No update to install");
  let total = 0;
  let done = 0;
  await u.downloadAndInstall((e) => {
    if (e.event === "Started") total = e.data.contentLength ?? 0;
    else if (e.event === "Progress") {
      done += e.data.chunkLength;
      if (total) onProgress(Math.min(100, Math.round((done / total) * 100)));
    } else onProgress(100);
  });
};

export const restartApp = () => relaunch();

export const openDownloadPage = () => openUrl(DOWNLOAD_PAGE).catch(() => {});
