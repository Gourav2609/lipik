/** "Me", "Them" or "Speaker N": a label that names nobody yet. Matches the
 * backend's `is_default_label`. */
export const isDefaultSpeaker = (speaker: string) => {
  const s = speaker.trim().toLowerCase();
  return s === "me" || s === "them" || /^speaker \d+$/.test(s);
};
