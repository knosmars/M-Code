import { useState, useRef, useEffect } from 'react';
import styles from './TitleBar.module.css';

interface TitleBarProps {
  onToggleSidebar?: () => void;
  onSearch?: () => void;
  onOpenSettings?: () => void;
  onToggleFiles?: () => void;
  onNewSession?: () => void;
  onCommandPalette?: () => void;
}

/**
 * Top window toolbar, styled after the Claude Code desktop client: a left
 * cluster of navigation icons (menu / sidebar toggle / search / back /
 * forward). The native OS window frame provides minimise / maximise / close,
 * so this toolbar deliberately renders no window-control buttons of its own.
 *
 * The hamburger opens a dropdown that hosts Settings and Files.
 */
export function TitleBar({
  onToggleSidebar,
  onSearch,
  onOpenSettings,
  onToggleFiles,
  onNewSession,
  onCommandPalette,
}: TitleBarProps) {
  const [menuOpen, setMenuOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!menuOpen) return;
    const onDocClick = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        setMenuOpen(false);
      }
    };
    const onEsc = (e: KeyboardEvent) => { if (e.key === 'Escape') setMenuOpen(false); };
    document.addEventListener('mousedown', onDocClick);
    document.addEventListener('keydown', onEsc);
    return () => {
      document.removeEventListener('mousedown', onDocClick);
      document.removeEventListener('keydown', onEsc);
    };
  }, [menuOpen]);

  const run = (fn?: () => void) => () => { setMenuOpen(false); fn?.(); };

  return (
    <header className={styles.toolbar}>
      <div className={`${styles.toolbarGroup} toolbar__group--left`}>
        <div className={styles.toolbarMenu} ref={menuRef}>
          <button
            type="button"
            className={`${styles.toolbarIconBtn}${menuOpen ? ' ' + styles['toolbarIconBtn--active'] : ''}`}
            aria-label="Menu"
            aria-haspopup="menu"
            aria-expanded={menuOpen}
            title="Menu"
            onClick={() => setMenuOpen((v) => !v)}
          >
            <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true">
              <path d="M2 4h12M2 8h12M2 12h12" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" />
            </svg>
          </button>
          {menuOpen && (
            <div className={styles.toolbarDropdown} role="menu">
              <button type="button" className={styles.toolbarDropdownItem} role="menuitem" onClick={run(onNewSession)}>
                <svg viewBox="0 0 16 16" width="15" height="15" fill="none" aria-hidden="true">
                  <path d="M8 3v10M3 8h10" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" />
                </svg>
                New session
                <span className={styles.toolbarDropdownShortcut}>⌘N</span>
              </button>
              <button type="button" className={styles.toolbarDropdownItem} role="menuitem" onClick={run(onToggleFiles)}>
                <svg viewBox="0 0 16 16" width="15" height="15" fill="none" aria-hidden="true">
                  <path d="M2 4.5h4l1.2 1.5H14v7H2V4.5z" stroke="currentColor" strokeWidth="1.3" strokeLinejoin="round" />
                </svg>
                Files
              </button>
              <button type="button" className={styles.toolbarDropdownItem} role="menuitem" onClick={run(onCommandPalette)}>
                <svg viewBox="0 0 16 16" width="15" height="15" fill="none" aria-hidden="true">
                  <rect x="2" y="3" width="12" height="10" rx="1.5" stroke="currentColor" strokeWidth="1.3" />
                  <path d="M5 7l2 1.5L5 10M9 10h2.5" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" strokeLinejoin="round" />
                </svg>
                Command palette
                <span className={styles.toolbarDropdownShortcut}>⌘K</span>
              </button>
              <button type="button" className={styles.toolbarDropdownItem} role="menuitem" onClick={run(onOpenSettings)}>
                <svg viewBox="0 0 16 16" width="15" height="15" fill="none" aria-hidden="true">
                  <circle cx="8" cy="8" r="2.3" stroke="currentColor" strokeWidth="1.3" />
                  <path d="M8 1.5v2M8 12.5v2M1.5 8h2M12.5 8h2M3.4 3.4l1.4 1.4M11.2 11.2l1.4 1.4M12.6 3.4l-1.4 1.4M4.8 11.2l-1.4 1.4" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" />
                </svg>
                Settings
              </button>
            </div>
          )}
        </div>

        <button
          type="button"
          className={styles.toolbarIconBtn}
          aria-label="Toggle sidebar"
          title="Toggle sidebar"
          onClick={onToggleSidebar}
        >
          <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true">
            <rect x="1.5" y="2.5" width="13" height="11" rx="2" stroke="currentColor" strokeWidth="1.3" fill="none" />
            <path d="M6 2.5v11" stroke="currentColor" strokeWidth="1.3" />
          </svg>
        </button>
        <button
          type="button"
          className={styles.toolbarIconBtn}
          aria-label="Search"
          title="Search"
          onClick={onSearch}
        >
          <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true">
            <circle cx="7" cy="7" r="4.5" stroke="currentColor" strokeWidth="1.3" fill="none" />
            <path d="M10.5 10.5L14 14" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" />
          </svg>
        </button>
        <button type="button" className={`${styles.toolbarIconBtn} ${styles['toolbarIconBtn--nav']}`} aria-label="Back" title="Back" disabled>
          <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true">
            <path d="M10 3L5 8l5 5" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" fill="none" />
          </svg>
        </button>
        <button type="button" className={`${styles.toolbarIconBtn} ${styles['toolbarIconBtn--nav']}`} aria-label="Forward" title="Forward" disabled>
          <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true">
            <path d="M6 3l5 5-5 5" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" fill="none" />
          </svg>
        </button>
      </div>
    </header>
  );
}
