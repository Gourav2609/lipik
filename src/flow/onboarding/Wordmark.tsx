import "@fontsource/martel/600.css";
import { APP_NAME } from "../brand";
import { LipikMark } from "../ui";

export const Wordmark = ({
  width = 160,
  className = "",
}: {
  width?: number;
  className?: string;
}) => {
  const size = Math.round(width / 6);
  return (
    <div
      className={`flex items-center justify-center gap-[0.35em] font-semibold leading-none text-text ${className}`}
      style={{ fontSize: size, fontFamily: '"Martel", Georgia, serif' }}
    >
      <LipikMark size={Math.round(size * 1.1)} />
      {APP_NAME}
    </div>
  );
};
