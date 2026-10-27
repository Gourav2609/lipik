import { useState } from "react";
import { isDefaultSpeaker } from "@/lib/speakers";

/** The inline editor for a speaker's name. Starts with the current name
 * selected (empty for "Speaker N"); Enter or leaving saves, Escape cancels.
 * `onDone` gets the trimmed new name, or null when nothing changed. */
export const SpeakerNameInput = ({
  speaker,
  placeholder,
  className,
  onDone,
}: {
  speaker: string;
  placeholder: string;
  className?: string;
  onDone: (name: string | null) => void;
}) => {
  const [value, setValue] = useState(isDefaultSpeaker(speaker) ? "" : speaker);
  const [done, setDone] = useState(false);
  const finish = (name: string | null) => {
    if (done) return;
    setDone(true);
    const clean = name?.trim();
    onDone(clean && clean !== speaker ? clean : null);
  };
  return (
    <input
      autoFocus
      value={value}
      placeholder={placeholder}
      aria-label="Speaker name"
      maxLength={60}
      className={className}
      onChange={(e) => setValue(e.target.value)}
      onFocus={(e) => e.currentTarget.select()}
      onBlur={() => finish(value)}
      onKeyDown={(e) => {
        if (e.key === "Enter") finish(value);
        if (e.key === "Escape") finish(null);
      }}
    />
  );
};
