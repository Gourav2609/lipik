import { useEffect, useState } from "react";
import { Check, Copy } from "lucide-react";
import { Button } from "../../ui";
import { copyToClipboard } from "@/components/settings/history/clipboard";

/** Copies `text` (a short stats summary) to the clipboard. */
export const CopyStats = ({ text }: { text: string }) => {
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    if (!copied) return;
    const timer = setTimeout(() => setCopied(false), 2000);
    return () => clearTimeout(timer);
  }, [copied]);

  const copy = async () => {
    if (await copyToClipboard(text)) setCopied(true);
  };

  return (
    <Button
      variant="secondary"
      size="sm"
      title="Copy a short summary of your stats"
      icon={
        copied ? (
          <Check size={16} strokeWidth={1.75} />
        ) : (
          <Copy size={16} strokeWidth={1.75} />
        )
      }
      onClick={copy}
    >
      {copied ? "Copied" : "Copy stats"}
    </Button>
  );
};
