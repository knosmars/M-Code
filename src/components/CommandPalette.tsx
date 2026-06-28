import { useState, useEffect, useRef, useCallback } from 'react';
import styles from './CommandPalette.module.css';

interface CommandItem {
  id: string;
  label: string;
  shortcut?: string;
  action: () => void;
}

interface Props {
  open: boolean;
  onClose: () => void;
  commands?: CommandItem[];
}

const DEFAULT_COMMANDS: CommandItem[] = [
  { id: 'new-chat', label: 'New Session', shortcut: '⌘N', action: () => {} },
  { id: 'focus-search', label: 'Focus Search', shortcut: '⌘K', action: () => {} },
  { id: 'close-tab', label: 'Close Tab', shortcut: '⌘W', action: () => {} },
  { id: 'cancel-stream', label: 'Cancel Streaming', shortcut: 'Esc', action: () => {} },
];

export function CommandPalette({ open, onClose, commands }: Props) {
  const [query, setQuery] = useState('');
  const [selectedIndex, setSelectedIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  const allCommands = commands ?? DEFAULT_COMMANDS;

  const filtered = allCommands.filter(
    (c) => !query || c.label.toLowerCase().includes(query.toLowerCase()),
  );

  const executeAction = useCallback(
    (item: CommandItem) => {
      item.action();
      onClose();
    },
    [onClose],
  );

  useEffect(() => {
    if (open) {
      setQuery('');
      setSelectedIndex(0);
      requestAnimationFrame(() => inputRef.current?.focus());
    }
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const handler = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        onClose();
      } else if (e.key === 'ArrowDown') {
        e.preventDefault();
        setSelectedIndex((prev) => Math.min(prev + 1, filtered.length - 1));
      } else if (e.key === 'ArrowUp') {
        e.preventDefault();
        setSelectedIndex((prev) => Math.max(prev - 1, 0));
      } else if (e.key === 'Enter') {
        e.preventDefault();
        const item = filtered[selectedIndex];
        if (item) executeAction(item);
      }
    };
    window.addEventListener('keydown', handler);
    return () => window.removeEventListener('keydown', handler);
  }, [open, filtered, selectedIndex, executeAction, onClose]);

  if (!open) return null;

  return (
    <div className={styles.overlay} onClick={onClose}>
      <div className={styles.palette} onClick={(e) => e.stopPropagation()}>
        <input
          ref={inputRef}
          className={styles.input}
          placeholder="Type a command or search..."
          value={query}
          onChange={(e) => {
            setQuery(e.target.value);
            setSelectedIndex(0);
          }}
        />
        <div className={styles.list}>
          {filtered.length === 0 ? (
            <div className={styles.empty}>No commands found</div>
          ) : (
            filtered.map((cmd, idx) => (
              <div
                key={cmd.id}
                className={`${styles.item}${idx === selectedIndex ? ' ' + styles['item--selected'] : ''}`}
                onClick={() => executeAction(cmd)}
              >
                <span className={styles.itemLabel}>{cmd.label}</span>
                {cmd.shortcut && (
                  <span className={styles.itemShortcut}>{cmd.shortcut}</span>
                )}
              </div>
            ))
          )}
        </div>
      </div>
    </div>
  );
}
