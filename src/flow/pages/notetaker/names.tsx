import type { MeetingDetail } from "@/bindings";

const GENERIC = new Set(["me", "them", "you", "others", "unknown contact"]);

/** Names of the people in a meeting: attendees and named speakers, plus first names. */
export const meetingNames = (detail: MeetingDetail | null) => {
  if (!detail) return [];
  const full = [
    ...detail.meeting.attendees.map((a) => a.name ?? ""),
    ...detail.segments.map((s) => s.speaker),
  ]
    .map((n) => n.trim())
    .filter((n) => n.length > 1 && !GENERIC.has(n.toLowerCase()));
  const first = full.map((n) => n.split(/\s+/)[0]).filter((n) => n.length > 1);
  return [...new Set([...full, ...first])].sort((a, b) => b.length - a.length);
};

const escape = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");

/** Text with the given names tinted. */
export const Mentions = ({
  text,
  names,
  className,
}: {
  text: string;
  names: string[];
  className: string;
}) => {
  if (!names.length) return <>{text}</>;
  const pattern = new RegExp(`\\b(${names.map(escape).join("|")})\\b`, "g");
  return (
    <>
      {text.split(pattern).map((part, i) =>
        i % 2 ? (
          <span key={i} className={className}>
            {part}
          </span>
        ) : (
          part
        ),
      )}
    </>
  );
};
