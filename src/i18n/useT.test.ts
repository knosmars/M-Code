// @vitest-environment jsdom
import { describe, it, expect, beforeEach } from 'vitest';
import { renderHook } from '@testing-library/react';
import { useSettingsStore } from '../stores/settingsStore';
import { useT } from './useT';

beforeEach(() => {
  useSettingsStore.getState().setLanguage('zh');
});

describe('useT interpolation', () => {
  // Unknown keys fall through to the key string itself; this lets us test the
  // {name} substitution mechanism directly without depending on later-task keys.
  it('substitutes {name} tokens in the resolved string', () => {
    const { result } = renderHook(() => useT());
    const t = result.current as (k: string, p?: Record<string, string | number>) => string;
    expect(t('{greeting} world', { greeting: 'hi' })).toBe('hi world');
    expect(t('count {n}', { n: 3 })).toBe('count 3'); // numeric param coerced
    expect(t('{a} {b}', { a: 'X' })).toBe('X {b}'); // unmatched token left as-is
  });

  it('returns the value unchanged when no params are given', () => {
    const { result } = renderHook(() => useT());
    const t = result.current as (k: string, p?: Record<string, string | number>) => string;
    expect(t('plain text')).toBe('plain text');
    // existing zero-arg lookups still work
    expect(typeof result.current('chat.tokens')).toBe('string');
  });
});
