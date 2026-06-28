/**
 * Per-session chat draft persistence.
 *
 * The composer input is kept in React state, which is lost on app restart or
 * crash. These helpers mirror it to localStorage keyed by session id so an
 * unsent message survives reloads and session switches.
 */

const PREFIX = 'meyatu-draft:';

/** Key used for the composer before any session exists (first message). */
export const NEW_SESSION_DRAFT_KEY = '__new__';

function keyFor(sessionId: string | null | undefined): string {
  return PREFIX + (sessionId || NEW_SESSION_DRAFT_KEY);
}

/** Persist (or clear, when blank) the draft for a session. */
export function saveDraft(sessionId: string | null | undefined, text: string): void {
  try {
    if (text.trim()) {
      localStorage.setItem(keyFor(sessionId), text);
    } else {
      localStorage.removeItem(keyFor(sessionId));
    }
  } catch {
    /* storage unavailable — non-fatal */
  }
}

/** Load the saved draft for a session, or '' if none. */
export function loadDraft(sessionId: string | null | undefined): string {
  try {
    return localStorage.getItem(keyFor(sessionId)) ?? '';
  } catch {
    return '';
  }
}

/** Remove the saved draft for a session (e.g. after sending). */
export function clearDraft(sessionId: string | null | undefined): void {
  try {
    localStorage.removeItem(keyFor(sessionId));
  } catch {
    /* storage unavailable — non-fatal */
  }
}
