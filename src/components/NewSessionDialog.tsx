import { useState, useCallback, useEffect } from 'react';
import { typedInvoke } from '../utils/ipc';
import styles from './NewSessionDialog.module.css';

interface NewSessionDialogProps {
  open: boolean;
  /** Called when the user dismisses without creating */
  onClose: () => void;
  /** Called with the chosen workspace path (empty string = no workspace) */
  onConfirm: (workspacePath: string | null) => void;
}

/**
 * New-session dialog that replaces the old "+" button.
 *
 * Flow:
 *  1. User sees Local / SSH choice.
 *  2. Local → recent folders list + "New folder…" at bottom.
 *  3. Choose a recent folder → onConfirm(path).
 *  4. Click "New folder…" → OS folder picker → onConfirm(path).
 *  5. SSH → creates without a workspace → onConfirm(null).
 */
export function NewSessionDialog({ open, onClose, onConfirm }: NewSessionDialogProps) {
  const [step, setStep] = useState<'choose' | 'local'>('choose');
  const [recentPaths, setRecentPaths] = useState<string[]>(() => {
    try {
      return JSON.parse(localStorage.getItem('meyatu_workspace_paths') ?? '[]');
    } catch { return []; }
  });

  const removeRecentPath = useCallback((path: string) => {
    setRecentPaths((prev) => {
      const next = prev.filter((x) => x !== path);
      localStorage.setItem('meyatu_workspace_paths', JSON.stringify(next));
      return next;
    });
  }, []);

  // Reset step when dialog opens/closes
  useEffect(() => { if (!open) setStep('choose'); }, [open]);

  const openFolderPicker = useCallback(async () => {
    try {
      const path = await typedInvoke<string | null>('tool_pick_folder');
      if (path) {
        onConfirm(path);
      }
    } catch {
      const path = window.prompt('Enter workspace path:');
      if (path) onConfirm(path);
    }
  }, [onConfirm]);

  const handleSelectRecent = useCallback((path: string) => {
    onConfirm(path);
  }, [onConfirm]);

  if (!open) return null;

  return (
    <>
      <div className={styles.overlay} onClick={onClose} />
      <div className={styles.dialog} role="dialog" aria-label="New session">
        {step === 'choose' && (
          <>
            <div className={styles.title}>New session</div>

            <button
              type="button"
              className={styles.choice}
              onClick={() => setStep('local')}
            >
              <svg viewBox="0 0 24 24" width="28" height="28" fill="none" aria-hidden="true">
                <rect x="3" y="5" width="18" height="14" rx="2" stroke="currentColor" strokeWidth="1.5" />
                <path d="M7 19v2M17 19v2M3 13h18" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
              </svg>
              <div className={styles.choiceText}>
                <strong>Local folder</strong>
                <span>Work on code from your computer</span>
              </div>
              <span className={styles.arrow}>→</span>
            </button>

            <button
              type="button"
              className={styles.choice}
              onClick={() => onConfirm(null)}
            >
              <svg viewBox="0 0 24 24" width="28" height="28" fill="none" aria-hidden="true">
                <path d="M4 17l4-4-4-4M10 19h10" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
                <circle cx="20" cy="7" r="3" stroke="currentColor" strokeWidth="1.5" />
              </svg>
              <div className={styles.choiceText}>
                <strong>SSH server</strong>
                <span>Connect to a remote workspace</span>
              </div>
              <span className={styles.arrow}>→</span>
            </button>
          </>
        )}

        {step === 'local' && (
          <>
            <div className={styles.title}>
              <button
                type="button"
                className={styles.back}
                onClick={() => setStep('choose')}
                aria-label="Back"
              >
                ←
              </button>
              Select a folder
            </div>

            <div className={styles.folderList}>
              {recentPaths.length > 0 ? (
                recentPaths.map((p) => (
                  <button
                    key={p}
                    type="button"
                    className={styles.folderItem}
                    onClick={() => handleSelectRecent(p)}
                  >
                    <svg viewBox="0 0 16 16" width="14" height="14" fill="none" aria-hidden="true">
                      <path d="M2 4.5h4l1.2 1.5H14v6.5H2V4.5z" stroke="currentColor" strokeWidth="1.2" strokeLinejoin="round" />
                    </svg>
                    <span className={styles.folderName}>{p.split(/[\\/]/).filter(Boolean).pop()}</span>
                    <span className={styles.folderPath}>{p}</span>
                    <button
                      type="button"
                      className={styles.folderDelete}
                      onClick={(e) => { e.stopPropagation(); removeRecentPath(p); }}
                      aria-label={`Remove ${p}`}
                      title="Remove from list"
                    >×</button>
                  </button>
                ))
              ) : (
                <div className={styles.empty}>
                  <span>No recent folders</span>
                </div>
              )}
            </div>

            <button
              type="button"
              className={styles.newFolder}
              onClick={openFolderPicker}
            >
              <svg viewBox="0 0 16 16" width="14" height="14" fill="none" aria-hidden="true">
                <path d="M8 3v10M3 8h10" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
              </svg>
              New folder...
            </button>

            <button
              type="button"
              className={styles.skip}
              onClick={() => onConfirm(null)}
            >
              Skip — no workspace folder
            </button>
          </>
        )}
      </div>
    </>
  );
}