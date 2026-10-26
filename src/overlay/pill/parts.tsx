// Own drawings, sized to the measured elements.

// Units are device px at 2x, origin at the capsule centre.
export const MicGlyph = () => (
  <svg width="14" height="17" viewBox="-14 -17 28 34" fill="currentColor">
    <rect x="-5.5" y="-16" width="10.06" height="19.9" rx="5" />
    <path
      d="M-10.36 0.92A10.15 10.05 0 0 0 9.56 0.92"
      fill="none"
      stroke="currentColor"
      strokeWidth="3.45"
      strokeLinecap="round"
    />
    <rect x="-2.05" y="8" width="3.2" height="7.5" />
  </svg>
);

export const RecordIcon = () => (
  <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
    <circle
      cx="7"
      cy="6.95"
      r="5.82"
      stroke="currentColor"
      strokeWidth="1.12"
    />
    <circle cx="7" cy="6.95" r="2.95" fill="currentColor" />
  </svg>
);

export const XGlyph = ({
  size = 8,
  stroke = 1.5,
}: {
  size?: number;
  stroke?: number;
}) => (
  <svg width={size} height={size} viewBox="0 0 8 8" fill="none">
    <path
      d="M1 1l6 6M7 1L1 7"
      stroke="currentColor"
      strokeWidth={(stroke * 8) / size}
      strokeLinecap="round"
    />
  </svg>
);

export const CheckGlyph = ({
  size = 9,
  stroke = 1.6,
}: {
  size?: number;
  stroke?: number;
}) => (
  <svg width={size} height={size} viewBox="0 0 9 9" fill="none">
    <path
      d="M1 4.8l2.3 2.3L8 1.9"
      stroke="currentColor"
      strokeWidth={(stroke * 9) / size}
      strokeLinecap="round"
      strokeLinejoin="round"
    />
  </svg>
);

const lerp = (a: number, b: number, k: number) => Math.round(a + (b - a) * k);
const ticks = (lead: number[], tail: number[]) =>
  Array.from({ length: 8 }, (_, i) => {
    const k = i / 7;
    const [r, g, b] = lead.map((c, j) => lerp(c, tail[j], k));
    const a = (-i * Math.PI) / 4;
    return { color: `rgb(${r},${g},${b})`, sin: Math.sin(a), cos: Math.cos(a) };
  });
const SAGE = ticks([0x9c, 0xc8, 0xae], [0x2e, 0x3d, 0x34]);
// Notetaker processing (notetaker1 117.8): grey, ≈12 pt.
const GREY = ticks([0x8e, 0x89, 0x80], [0x32, 0x30, 0x2c]);

/** 8 ticks; the whole glyph steps 45° clockwise every 133 ms (in CSS). */
export const Spinner = ({ grey = false }: { grey?: boolean }) => {
  const size = grey ? 12 : 15;
  const [r0, r1] = grey ? [2.6, 5.3] : [3.4, 6.6];
  return (
    <svg
      className="spinner"
      width={size}
      height={size}
      viewBox={`${-size / 2} ${-size / 2} ${size} ${size}`}
    >
      {(grey ? GREY : SAGE).map((tk, i) => (
        <line
          key={i}
          x1={tk.sin * r0}
          y1={-tk.cos * r0}
          x2={tk.sin * r1}
          y2={-tk.cos * r1}
          stroke={tk.color}
          strokeWidth={grey ? 1.4 : 1.5}
          strokeLinecap="round"
        />
      ))}
    </svg>
  );
};

/** "<" on the chevron tab and under the Notetaker pill: 4 x 8 pt, 1.25 pt stroke. */
export const ChevronGlyph = () => (
  <svg className="chev" width="6" height="10" viewBox="0 0 6 10" fill="none">
    <path
      d="M4.4 1.4L1.6 5l2.8 3.6"
      stroke="currentColor"
      strokeWidth="1.25"
      strokeLinecap="round"
      strokeLinejoin="round"
    />
  </svg>
);
