const RUN_AGAIN_EVENT = "lipik:run-setup-again";

/** Shows setup again without touching the chosen model or other settings. */
export const runSetupAgain = () =>
  window.dispatchEvent(new Event(RUN_AGAIN_EVENT));

export const onRunSetupAgain = (show: () => void) => {
  window.addEventListener(RUN_AGAIN_EVENT, show);
  return () => window.removeEventListener(RUN_AGAIN_EVENT, show);
};
