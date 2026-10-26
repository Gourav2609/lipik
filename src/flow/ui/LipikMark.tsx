import type { SVGProps } from "react";

export interface LipikMarkProps extends SVGProps<SVGSVGElement> {
  size?: number;
  /** Draw the mark on the ink tile used for the app icon. */
  tile?: boolean;
}

const STROKES = "M6.5 6.4v4.6M10.5 6.4v11.6M14.5 6.4v7.4M18.5 6.4v3";

export const LipikMark = ({
  size = 22,
  tile = false,
  ...rest
}: LipikMarkProps) => (
  <svg
    width={size}
    height={size}
    viewBox="0 0 24 24"
    fill="none"
    aria-hidden="true"
    {...rest}
  >
    {tile && <rect width="24" height="24" rx="6.5" fill="#285a44" />}
    <g
      stroke={tile ? "#fbf9f4" : "currentColor"}
      strokeLinecap="round"
      transform={tile ? "translate(2.4 3.2) scale(0.8)" : undefined}
    >
      <path d="M3 5.4h18" strokeWidth="2.6" />
      <path d={STROKES} strokeWidth="2.3" />
    </g>
  </svg>
);
