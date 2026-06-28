import slash from './slash.module.css';

/** Slash-command dropdown shown above the composer when the input starts `/`. */
export function SlashCommandMenu<K extends string>({
  items,
  activeIndex,
  onSelect,
  onHover,
  t,
}: {
  items: ReadonlyArray<{ command: string; labelKey: K }>;
  activeIndex: number;
  onSelect: (index: number) => void;
  onHover: (index: number) => void;
  t: (key: K) => string;
}) {
  return (
    <div className={slash.slashMenu}>
      {items.map((cmd, i) => (
        <button
          key={cmd.command}
          type="button"
          className={`${slash.slashMenuItem}${i === activeIndex ? ' ' + slash['slashMenuItem--active'] : ''}`}
          onMouseDown={(e) => { e.preventDefault(); onSelect(i); }}
          onMouseEnter={() => onHover(i)}
        >
          <span className={slash.slashMenuCommand}>{cmd.command}</span>
          <span className={slash.slashMenuDesc}>{t(cmd.labelKey)}</span>
        </button>
      ))}
    </div>
  );
}
