import type { CSSProperties } from "react";
import type { Person } from "./data";

const TONES: [string, string][] = [
  ["var(--fk-accent-soft)", "var(--fk-accent)"],
  ["var(--fk-surface-2)", "var(--fk-text)"],
  ["var(--fk-active)", "var(--fk-accent-hover)"],
];

const hash = (s: string) =>
  [...s].reduce((h, c) => (h * 31 + c.charCodeAt(0)) >>> 0, 7);

const faceStyle = (person: Person): CSSProperties => {
  if (person.avatarUrl)
    return {
      backgroundImage: `url(${person.avatarUrl})`,
      backgroundSize: "cover",
      backgroundPosition: "center",
    };
  const [bg, fg] = TONES[hash(person.name) % TONES.length];
  return { background: bg, color: fg };
};

const letter = (person: Person) => person.name.trim().charAt(0).toUpperCase();

export const Avatar = ({
  person,
  size,
  className = "",
  style,
}: {
  person: Person;
  size: number;
  className?: string;
  style?: CSSProperties;
}) => (
  <span
    className={`flex shrink-0 items-center justify-center overflow-hidden rounded-full font-semibold leading-none ${className}`}
    style={{
      width: size,
      height: size,
      fontSize: Math.round(size * 0.5),
      ...faceStyle(person),
      ...style,
    }}
  >
    {!person.avatarUrl && letter(person)}
  </span>
);

/** Overlapping circles, the viewer left out, with a "+N" for the rest. */
export const AvatarStack = ({
  people,
  max = 3,
  size = 22,
  ring = "var(--nt-ring, var(--fk-panel))",
}: {
  people: Person[];
  max?: number;
  size?: number;
  ring?: string;
}) => {
  const others = people.filter((p) => !p.isYou);
  if (!others.length) return null;
  const shown = others.slice(0, max);
  const rest = others.length - shown.length;
  return (
    <span className="flex shrink-0 items-center">
      {shown.map((p, i) => (
        <Avatar
          key={p.id}
          person={p}
          size={size}
          style={{
            marginLeft: i ? -6 : 0,
            boxShadow: `0 0 0 2px ${ring}`,
          }}
        />
      ))}
      {rest > 0 && (
        <span className="fk-num ml-[6px] text-[12px] text-[var(--fk-text-muted)]">
          +{rest}
        </span>
      )}
    </span>
  );
};
