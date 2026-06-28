// @vitest-environment jsdom
import { describe, it, expect, beforeEach } from 'vitest';
import { useSettingsStore } from './settingsStore';

describe('settingsStore autoSemanticIndex', () => {
  beforeEach(() => {
    useSettingsStore.getState().setAutoSemanticIndex(false);
  });

  it('exposes autoSemanticIndex defaulting to false', () => {
    expect(useSettingsStore.getState().autoSemanticIndex).toBe(false);
  });

  it('setAutoSemanticIndex toggles the flag', () => {
    useSettingsStore.getState().setAutoSemanticIndex(true);
    expect(useSettingsStore.getState().autoSemanticIndex).toBe(true);
  });

  it('persists the flag to localStorage', () => {
    useSettingsStore.getState().setAutoSemanticIndex(true);
    const raw = JSON.parse(localStorage.getItem('meyatu-settings')!);
    expect(raw.autoSemanticIndex).toBe(true);
  });
});
