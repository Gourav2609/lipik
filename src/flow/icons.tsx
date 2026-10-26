import type { SVGProps } from "react";

// Own drawings for the few shell icons that have no Lucide equivalent.
// Same grid and stroke as Lucide (24 viewBox, 2 stroke, round caps).

interface IconProps extends SVGProps<SVGSVGElement> {
  size?: number | string;
}

const Svg = ({ size = 24, children, ...rest }: IconProps) => (
  <svg
    width={size}
    height={size}
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth={2}
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
    {...rest}
  >
    {children}
  </svg>
);

export const RecordIcon = (p: IconProps) => (
  <Svg {...p}>
    <circle cx="12" cy="12" r="10" />
    <circle cx="12" cy="12" r="4.5" fill="currentColor" stroke="none" />
  </Svg>
);
