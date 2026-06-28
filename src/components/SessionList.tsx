import { useState, useRef, useCallback, useEffect } from 'react';
import type { Session } from '../types/session';
import type { ProviderConfig } from '../types/provider';
import styles from './SessionList.module.css';

interface SessionListProps {
  sessions: Session[];
  currentSessionId: string | null;
  providers: ProviderConfig[];
  onSelect: (id: string) => void;
  onNew: () => void;
  onDelete: (id: string) => void;
  onRename: (id: string, title: string) => void;
  onCustomize?: () => void;
  focusSearchKey?: number;
  /** Which pane is active: sessions or workspace files */
  sidebarMode: 'sessions' | 'files';
  /** Called when user switches the sidebar pane */
  onToggleSidebarMode: (mode: 'sessions' | 'files') => void;
}

/** Split text by query for highlighting. */
function highlightMatches(text: string, query: string): { text: string; match: boolean }[] {
  if (!query.trim()) return [{ text, match: false }];
  const parts: { text: string; match: boolean }[] = [];
  const lower = text.toLowerCase();
  const q = query.toLowerCase();
  let last = 0;
  for (let i = 0; i <= lower.length - q.length; i++) {
    if (lower.slice(i, i + q.length) === q) {
      if (i > last) parts.push({ text: text.slice(last, i), match: false });
      parts.push({ text: text.slice(i, i + q.length), match: true });
      last = i + q.length;
      i += q.length - 1;
    }
  }
  if (last < text.length) parts.push({ text: text.slice(last), match: false });
  return parts;
}

/** A small status glyph rendered to the left of each recent session. */
function SessionDot({ active }: { active: boolean }) {
  return (
    <span className={`${styles.navRecentsDot}${active ? ' ' + styles['navRecentsDot--active'] : ''}`} aria-hidden="true" />
  );
}

/**
 * Left navigation rail, styled after the Claude Code desktop client:
 * a Plan / Code mode toggle, primary nav actions, and a "Recents" list
 * of sessions with inline rename and search.
 */
export function SessionList({
  sessions,
  currentSessionId,
  providers,
  onSelect,
  onNew,
  onDelete,
  onRename,
  onCustomize,
  focusSearchKey,
  sidebarMode,
  onToggleSidebarMode,
}: SessionListProps) {
  const [search, setSearch] = useState('');
  const [searchOpen, setSearchOpen] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editingTitle, setEditingTitle] = useState('');
  const editRef = useRef<HTMLInputElement>(null);
  const searchRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (editingId !== null) {
      editRef.current?.focus();
      editRef.current?.select();
    }
  }, [editingId]);

  useEffect(() => {
    if (focusSearchKey && focusSearchKey > 0) {
      setSearchOpen(true);
      setTimeout(() => searchRef.current?.focus(), 0);
    }
  }, [focusSearchKey]);

  useEffect(() => {
    if (!searchOpen) return;
    const handleClick = (e: MouseEvent) => {
      const el = searchRef.current?.closest(`.${styles.navSearch}`);
      if (el && !el.contains(e.target as Node)) {
        setSearchOpen(false);
        setSearch('');
      }
    };
    document.addEventListener('mousedown', handleClick);
    return () => document.removeEventListener('mousedown', handleClick);
  }, [searchOpen]);

  const commitRename = useCallback(
    (id: string) => {
      const trimmed = editingTitle.trim();
      if (trimmed.length > 0 && trimmed.length <= 80) onRename(id, trimmed);
      setEditingId(null);
      setEditingTitle('');
    },
    [editingTitle, onRename],
  );

  const cancelRename = useCallback(() => {
    setEditingId(null);
    setEditingTitle('');
  }, []);

  // Keep the provider list referenced so the prop stays meaningful even though
  // the Claude Code rail no longer renders a provider badge per row.
  void providers;

  const filtered = search.trim()
    ? sessions.filter((s) => s.title.toLowerCase().includes(search.toLowerCase()))
    : sessions;
  const sorted = [...filtered].sort((a, b) => b.updatedAt - a.updatedAt);

  return (
    <nav className={styles.nav}>
      {/* Sidebar pane toggle — Sessions / Files */}
      <div className={styles.navToggle} role="tablist" aria-label="Sidebar pane">
        <button
          type="button"
          className={`${styles.navToggleBtn}${sidebarMode === 'sessions' ? ' ' + styles['navToggleBtn--active'] : ''}`}
          onClick={() => onToggleSidebarMode('sessions')}
          aria-label="Sessions"
          role="tab"
          aria-selected={sidebarMode === 'sessions'}
        >
          <svg viewBox="0 0 16 16" width="14" height="14" fill="none" aria-hidden="true">
            <rect x="2" y="3" width="12" height="8" rx="1" stroke="currentColor" strokeWidth="1.2" />
            <path d="M5.5 13.5h5" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" />
          </svg>
        </button>
        <button
          type="button"
          className={`${styles.navToggleBtn} ${styles['navToggleBtn--code']}${sidebarMode === 'files' ? ' ' + styles['navToggleBtn--active'] : ''}`}
          onClick={() => onToggleSidebarMode('files')}
          role="tab"
          aria-selected={sidebarMode === 'files'}
        >
          <svg viewBox="0 0 16 16" width="14" height="14" fill="none" aria-hidden="true">
            <path d="M2 4.5h4l1.2 1.5H14v6.5H2V4.5z" stroke="currentColor" strokeWidth="1.2" strokeLinejoin="round" />
          </svg>
          Files
        </button>
      </div>

      {/* Primary nav actions */}
      <div className={styles.navActions}>
        <button type="button" className={styles.navItem} onClick={onNew}>
          <svg className={styles.navItemIcon} viewBox="0 0 16 16" width="15" height="15" fill="none" aria-hidden="true">
            <path d="M8 3v10M3 8h10" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" />
          </svg>
          New session
        </button>
        <button type="button" className={styles.navItem} onClick={onCustomize}>
          <svg className={styles.navItemIcon} viewBox="0 0 16 16" width="15" height="15" fill="none" aria-hidden="true">
            <rect x="2.5" y="5" width="11" height="8" rx="1.5" stroke="currentColor" strokeWidth="1.3" />
            <path d="M6 5V4a2 2 0 0 1 4 0v1" stroke="currentColor" strokeWidth="1.3" />
          </svg>
          Customize
        </button>
      </div>

      {/* Recents header */}
      <div className={styles.navRecentsHeader}>
        <span className={styles.navRecentsTitle}>Recents</span>
        <button
          type="button"
          className={styles.navRecentsSort}
          onClick={() => setSearchOpen((v) => !v)}
          aria-label="Search / sort sessions"
          title="Search sessions"
        >
          <svg viewBox="0 0 16 16" width="14" height="14" fill="none" aria-hidden="true">
            <path d="M3 5h10M5 8h6M7 11h2" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" />
          </svg>
        </button>
      </div>

      {searchOpen && (
        <div className={styles.navSearch}>
          <input
            ref={searchRef}
            type="text"
            className={styles.navSearchInput}
            placeholder="Search sessions…"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === 'Escape') {
                setSearch('');
                setSearchOpen(false);
              }
            }}
            aria-label="Search sessions"
          />
        </div>
      )}

      <div className={styles.navRecentsList}>
        {sorted.length === 0 ? (
          <div className={styles.navRecentsEmpty}>
            {search.trim() ? 'No matches' : 'No sessions yet'}
          </div>
        ) : (
          sorted.map((session) => {
            const isActive = session.id === currentSessionId;
            const isEditing = editingId === session.id;

            return (
              <div
                key={session.id}
                className={`${styles.navRecentsItem}${isActive ? ' ' + styles['navRecentsItem--active'] : ''}`}
                onClick={() => {
                  if (!isEditing) onSelect(session.id);
                }}
              >
                <SessionDot active={isActive} />
                {isEditing ? (
                  <input
                    ref={editRef as React.RefObject<HTMLInputElement>}
                    className={styles.navRecentsRename}
                    value={editingTitle}
                    onChange={(e) => setEditingTitle(e.target.value)}
                    onBlur={() => commitRename(session.id)}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter') commitRename(session.id);
                      if (e.key === 'Escape') cancelRename();
                    }}
                    onClick={(e) => e.stopPropagation()}
                    maxLength={80}
                    aria-label={`Rename session ${session.title}`}
                  />
                ) : (
                  <span
                    className={`${styles.navRecentsLabel} truncate`}
                    onDoubleClick={(e) => {
                      e.stopPropagation();
                      setEditingId(session.id);
                      setEditingTitle(session.title);
                    }}
                    title={session.title}
                  >
                    {search.trim()
                      ? highlightMatches(session.title, search).map((part, i) =>
                          part.match ? (
                            <mark key={i} className={styles.navSearchHighlight}>{part.text}</mark>
                          ) : (
                            <span key={i}>{part.text}</span>
                          ),
                        )
                      : session.title}
                  </span>
                )}
                <button
                  type="button"
                  className={styles.navRecentsDelete}
                  onClick={(e) => {
                    e.stopPropagation();
                    onDelete(session.id);
                  }}
                  aria-label={`Delete session ${session.title}`}
                >
                  ×
                </button>
              </div>
            );
          })
        )}
      </div>
    </nav>
  );
}
