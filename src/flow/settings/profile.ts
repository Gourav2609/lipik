import { useCallback, useMemo } from "react";
import { isFlowPreview } from "../dev/preview";
import { localStore, useStoredRaw } from "./parts";

export interface Profile {
  firstName: string;
  lastName: string;
  email: string;
  /** Data URL of the chosen picture, or "" for the initial-letter avatar. */
  picture: string;
}

const KEY = "lipik.profile";

const EMPTY: Profile = { firstName: "", lastName: "", email: "", picture: "" };
const SAMPLE: Profile = {
  firstName: "Asha",
  lastName: "",
  email: "asha.rao@example.com",
  picture: "",
};

const parse = (raw: string | null): Profile => {
  const fallback = isFlowPreview ? SAMPLE : EMPTY;
  if (!raw) return fallback;
  try {
    return { ...fallback, ...JSON.parse(raw) };
  } catch {
    return fallback;
  }
};

/** Local-only user profile (Lipik has no account backend), shared by every
 *  component that reads it. */
export const useProfile = () => {
  const raw = useStoredRaw(KEY);
  const profile = useMemo(() => parse(raw), [raw]);

  const save = useCallback(
    (next: Profile) => localStore.set(KEY, JSON.stringify(next)),
    [],
  );
  const clear = useCallback(
    () => localStore.set(KEY, JSON.stringify(EMPTY)),
    [],
  );

  return { profile, save, clear };
};
