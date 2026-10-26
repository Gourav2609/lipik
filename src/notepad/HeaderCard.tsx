import { useEffect, useState, type ReactNode } from "react";
import { Search, X } from "lucide-react";

type Props = {
  icon: ReactNode;
  label: ReactNode;
  query: string;
  onQuery: (q: string) => void;
  placeholder: string;
  children?: ReactNode;
};

/** The ruled line on top of the transcript, with in-tab search. */
export const HeaderCard = ({
  icon,
  label,
  query,
  onQuery,
  placeholder,
  children,
}: Props) => {
  const [open, setOpen] = useState(false);
  const [index, setIndex] = useState(0);
  const [count, setCount] = useState(0);

  useEffect(() => setIndex(0), [query]);

  const hits = () => [
    ...document.querySelectorAll<HTMLElement>(".np-pane mark.np-hit"),
  ];

  useEffect(() => {
    const all = hits();
    setCount(all.length);
    all.forEach((h, i) => h.classList.toggle("now", i === index));
  });

  useEffect(() => {
    hits()[index]?.scrollIntoView({ block: "center", behavior: "smooth" });
  }, [query, index]);

  const close = () => {
    setOpen(false);
    onQuery("");
  };

  return (
    <div className="np-card">
      <div className="np-card-left">
        {open ? (
          <>
            <Search size={16} strokeWidth={1.75} />
            <input
              autoFocus
              value={query}
              placeholder={placeholder}
              aria-label={placeholder}
              onChange={(e) => onQuery(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Escape") close();
                if (e.key === "Enter" && count) {
                  setIndex((i) =>
                    e.shiftKey ? (i - 1 + count) % count : (i + 1) % count,
                  );
                }
              }}
            />
            {query.length >= 2 && (
              <span className="np-hitcount">
                {count ? `${index + 1} of ${count}` : "No matches"}
              </span>
            )}
          </>
        ) : (
          <>
            {icon}
            {label}
          </>
        )}
      </div>
      <div className="np-card-tools">
        <button
          type="button"
          className={`np-tool${open ? " on" : ""}`}
          aria-label={open ? "Close search" : "Search"}
          title={open ? "Close search" : "Search"}
          onClick={() => (open ? close() : setOpen(true))}
        >
          {open ? (
            <X size={16} strokeWidth={1.75} />
          ) : (
            <Search size={16} strokeWidth={1.75} />
          )}
        </button>
        {children}
      </div>
    </div>
  );
};
