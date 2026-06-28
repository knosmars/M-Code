import { describe, it, expect, beforeEach } from 'vitest';
import { saveDraft, loadDraft, clearDraft } from './draftStore';

function mockLocalStorage(): Storage {
  const store = new Map<string, string>();
  return {
    getItem: (k: string) => store.get(k) ?? null,
    setItem: (k: string, v: string) => void store.set(k, v),
    removeItem: (k: string) => void store.delete(k),
    clear: () => store.clear(),
    key: () => null,
    length: 0,
  } as unknown as Storage;
}

beforeEach(() => {
  (globalThis as unknown as { localStorage: Storage }).localStorage = mockLocalStorage();
});

describe('draftStore', () => {
  it('roundtrips a draft for a session', () => {
    saveDraft('s1', 'hello');
    expect(loadDraft('s1')).toBe('hello');
  });

  it('isolates drafts by session', () => {
    saveDraft('s1', 'one');
    saveDraft('s2', 'two');
    expect(loadDraft('s1')).toBe('one');
    expect(loadDraft('s2')).toBe('two');
  });

  it('blank/whitespace text clears the draft', () => {
    saveDraft('s1', 'x');
    saveDraft('s1', '   ');
    expect(loadDraft('s1')).toBe('');
  });

  it('clearDraft removes the saved draft', () => {
    saveDraft('s1', 'x');
    clearDraft('s1');
    expect(loadDraft('s1')).toBe('');
  });

  it('null and undefined session share the new-session key', () => {
    saveDraft(null, 'draft');
    expect(loadDraft(undefined)).toBe('draft');
  });

  it('missing draft returns empty string', () => {
    expect(loadDraft('nope')).toBe('');
  });
});
