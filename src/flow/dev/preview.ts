import { installMockTauri } from "./mockTauri";

const params = new URLSearchParams(window.location.search);

/** Plain-browser preview of the Flow UI (no Tauri), for screenshots. */
export const isFlowPreview =
  !("__TAURI_INTERNALS__" in window) && params.get("flowPreview") === "1";

/** ?look=<a|b|/** The window style from src/flow/looks; ?look=<id> previews another one. */
export const LOOK = (isFlowPreview && params.get("look")) || "d";

export const skipOnboarding =
  isFlowPreview && params.get("skipOnboarding") === "1";

if (isFlowPreview) {
  const theme = params.get("theme") === "light" ? "light" : "dark";
  installMockTauri({ theme });
}

/** ?onboarding=<step> shows that first-run onboarding step instead of the app. */
export const onboardingPreviewStep = isFlowPreview
  ? params.get("onboarding")
  : null;

/** ?scroll=<pt> scrolls the main content panel once the page has rendered. */
if (isFlowPreview && params.get("scroll")) {
  const top = Number(params.get("scroll")) || 0;
  const apply = (tries: number) => {
    const el = document.querySelector<HTMLElement>("main .fk-scroll");
    if (el && el.scrollHeight > el.clientHeight) el.scrollTop = top;
    else if (tries > 0) setTimeout(() => apply(tries - 1), 100);
  };
  window.addEventListener("load", () => setTimeout(() => apply(30), 300));
}
