import { useEffect, useState } from "react";
import type { Components } from "react-markdown";
import { MarkdownContent, openSafeUrl } from "@/components/whats-new";
import type { ReleaseNote } from "@/components/whats-new/releaseNotes";
import { MENU_EXIT_MS } from "./TopBar";
import { Button } from "./ui";
import "./overlays/overlays.css";

const body = "text-[15px] leading-[24px] text-[var(--fk-text-muted)]";

const heading: Components["h3"] = ({ children }) => (
  <h3 className="fk-display !mt-[24px] text-[18px] leading-[26px] text-[var(--fk-text)] first:!mt-0">
    {children}
  </h3>
);

const COMPONENTS: Components = {
  h1: heading,
  h2: heading,
  h3: heading,
  p: ({ children }) => <p className={body}>{children}</p>,
  strong: ({ children }) => (
    <strong className="font-semibold text-[var(--fk-text)]">{children}</strong>
  ),
  ul: ({ children }) => (
    <ul className={`list-disc space-y-[4px] ps-[20px] ${body}`}>{children}</ul>
  ),
  ol: ({ children }) => (
    <ol className={`list-decimal space-y-[4px] ps-[20px] ${body}`}>
      {children}
    </ol>
  ),
  li: ({ children }) => (
    <li className="ps-[2px] marker:text-[var(--fk-text-faint)]">{children}</li>
  ),
  a: ({ children, href }) => (
    <a
      href={href}
      rel="noreferrer"
      onClick={(e) => {
        e.preventDefault();
        if (href) void openSafeUrl(href);
      }}
      className="text-[var(--fk-accent)] underline decoration-[var(--fk-accent)]/40 underline-offset-2"
    >
      {children}
    </a>
  ),
};

/** The notes for the version the app just updated to; "Got it" marks them seen. */
export const WhatsNewSheet = ({
  note,
  version,
  onDismiss,
}: {
  note: ReleaseNote;
  version: string;
  onDismiss: () => void;
}) => {
  const [closing, setClosing] = useState(false);

  useEffect(() => {
    if (!closing) return;
    const id = setTimeout(onDismiss, MENU_EXIT_MS);
    return () => clearTimeout(id);
  }, [closing, onDismiss]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !e.defaultPrevented) setClosing(true);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <div
      className="absolute inset-0 z-40 flex items-center justify-center"
      data-fk-closing={closing || undefined}
    >
      <div className="fk-scrim-in absolute inset-0 bg-[var(--fk-scrim)]" />
      <div
        role="dialog"
        aria-label="What's new"
        className="fk-dialog relative flex max-h-[calc(100%-64px)] w-[min(560px,calc(100%-64px))] select-text flex-col overflow-hidden rounded-[var(--fk-r-xl)] bg-[var(--fk-panel)] shadow-[var(--fk-elev-3)]"
      >
        <div className="px-[32px] pb-[16px] pt-[28px]">
          <div className="fk-eyebrow">Updated to {version}</div>
          <h2 className="fk-display mt-[4px] text-[28px] leading-[36px] text-[var(--fk-text)]">
            What's new
          </h2>
        </div>
        <div className="min-h-0 flex-1 overflow-y-auto px-[32px]">
          <MarkdownContent
            markdown={note.markdown}
            components={COMPONENTS}
            className="space-y-[10px] [&_h3+*]:!mt-[6px]"
          />
        </div>
        <div className="flex justify-end px-[32px] pb-[24px] pt-[20px]">
          <Button onClick={() => setClosing(true)} className="min-w-[120px]">
            Got it
          </Button>
        </div>
      </div>
    </div>
  );
};
