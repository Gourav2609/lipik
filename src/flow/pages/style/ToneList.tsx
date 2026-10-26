import { ListGroup } from "../../ui";
import { Choice } from "./Choice";
import { CONTEXTS, SAMPLES, STYLES } from "./data";
import { useStyleStore } from "./store";

const OPTIONS = STYLES.map((s) => ({ id: s.id, name: s.name, hint: s.rule }));

export const ToneList = () => {
  const styles = useStyleStore((s) => s.styles);
  const setStyle = useStyleStore((s) => s.setStyle);

  return (
    <section aria-labelledby="tone-by-app">
      <h2 id="tone-by-app" className="text-[16px] font-semibold leading-[22px]">
        By app
      </h2>
      <p className="mt-[2px] text-[14px] leading-[20px] text-[var(--fk-text-muted)]">
        The line in the middle shows how a dictation comes out.
      </p>
      <ListGroup className="mt-[16px]">
        {CONTEXTS.map((c) => (
          <div
            key={c.id}
            className="grid grid-cols-[168px_minmax(0,1fr)_auto] items-start gap-[24px] border-b border-[var(--fk-border)] px-[12px] py-[16px]"
          >
            <div>
              <div className="text-[15px] font-semibold leading-[22px]">{c.label}</div>
              <div className="mt-[2px] text-[13px] leading-[18px] text-[var(--fk-text-muted)]">
                {c.apps}
              </div>
            </div>
            <p className="whitespace-pre-line border-l-2 border-[var(--fk-accent)] pl-[12px] text-[14px] leading-[21px]">
              {SAMPLES[c.id][styles[c.id]]}
            </p>
            <Choice
              label={`Tone for ${c.label.toLowerCase()}`}
              options={OPTIONS}
              value={styles[c.id]}
              onChange={(id) => setStyle(c.id, id)}
            />
          </div>
        ))}
      </ListGroup>
    </section>
  );
};
