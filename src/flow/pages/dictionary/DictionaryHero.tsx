import { ArrowRight } from "lucide-react";
import { APP_NAME } from "../../brand";
import { HeroBanner, HeroButton } from "../../ui";

const EXAMPLES: [string, string?][] = [
  ["Ananya Iyer"],
  ["k8s", "Kubernetes"],
  ["Thiruvananthapuram"],
];

const RuledExample = () => (
  <div className="hidden h-full items-center justify-end pr-[56px] xl:flex">
    <div className="w-[260px] border-t border-[color-mix(in_srgb,var(--fk-accent)_22%,transparent)]">
      {EXAMPLES.map(([word, replacement]) => (
        <div
          key={word}
          className="flex h-[36px] items-center gap-[6px] border-b border-[color-mix(in_srgb,var(--fk-accent)_22%,transparent)] px-[4px] text-[14.5px]"
        >
          {replacement ? (
            <>
              <span className="text-[var(--fk-text-muted)]">{word}</span>
              <ArrowRight
                size={14}
                strokeWidth={1.75}
                className="text-[var(--fk-text-faint)]"
              />
              <span className="font-medium text-[var(--fk-accent)]">
                {replacement}
              </span>
            </>
          ) : (
            <span className="font-medium text-[var(--fk-accent)]">{word}</span>
          )}
        </div>
      ))}
    </div>
  </div>
);

export const DictionaryHero = ({
  onAddNew,
  onClose,
  className,
}: {
  onAddNew: () => void;
  onClose: () => void;
  className?: string;
}) => (
  <HeroBanner
    className={className}
    art={<RuledExample />}
    onClose={onClose}
    title="Spell names your way"
    subtitle={`Add a word once and ${APP_NAME} writes it your way from then on.`}
    actions={
      <HeroButton strong onClick={onAddNew}>
        Add word
      </HeroButton>
    }
  />
);
