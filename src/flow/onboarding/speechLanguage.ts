export const DEFAULT_MODEL_ID = "parakeet-unified-en-0.6b";
/** 25 European languages, about as fast as the English model. */
export const EUROPEAN_MODEL_ID = "parakeet-tdt-0.6b-v3";
/** About 100 languages; slower. */
export const WORLD_MODEL_ID = "whisper-large-v3-turbo";

/** Indian languages: these Macs stay on English for Hinglish and Apple's en-IN engine. */
const INDIAN = ["hi", "mr", "ta", "te", "bn", "gu", "kn", "ml", "pa", "ur"];

/** The language part of a macOS language tag: "en-IN" → "en". */
export const baseLanguage = (tag: string | null | undefined) =>
  (tag || "en").toLowerCase().split(/[-_]/)[0];

const isIndian = (tag: string) => {
  const parts = tag.toLowerCase().split(/[-_]/);
  return INDIAN.includes(parts[0]) || parts.slice(1).includes("in");
};

/** The speech model for the Mac's language, among the models on offer. */
export const modelForLanguage = (
  tag: string | null | undefined,
  models: { id: string; supported_languages: string[] }[],
): string => {
  const lang = baseLanguage(tag);
  if (lang === "en" || isIndian(tag ?? "")) return DEFAULT_MODEL_ID;
  const offered = (id: string) => models.find((m) => m.id === id);
  if (offered(EUROPEAN_MODEL_ID)?.supported_languages.includes(lang))
    return EUROPEAN_MODEL_ID;
  if (offered(WORLD_MODEL_ID)) return WORLD_MODEL_ID;
  return DEFAULT_MODEL_ID;
};
