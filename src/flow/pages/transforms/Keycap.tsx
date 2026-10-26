import { Kbd } from "../../ui";

export const OptKeys = ({ keyLabel, size }: { keyLabel: string; size?: "md" | "lg" }) => (
  <span className="inline-flex items-center gap-[4px] align-middle">
    <Kbd size={size}>⌥</Kbd>
    <Kbd size={size}>{keyLabel.toUpperCase()}</Kbd>
  </span>
);
