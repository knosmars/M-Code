// @vitest-environment jsdom
import { describe, it, expect, beforeEach } from 'vitest';
import { render, screen } from '@testing-library/react';
import { useSettingsStore } from '../stores/settingsStore';
import { TokenDashboard } from './TokenDashboard';

beforeEach(() => {
  useSettingsStore.getState().setLanguage('zh');
});

describe('TokenDashboard i18n', () => {
  it('shows Chinese heading by default', () => {
    render(<TokenDashboard />);
    expect(screen.getByText('当前会话')).toBeTruthy();
  });

  it('shows English heading when language is en', () => {
    useSettingsStore.getState().setLanguage('en');
    render(<TokenDashboard />);
    expect(screen.getByText('Current session')).toBeTruthy();
    useSettingsStore.getState().setLanguage('zh');
  });
});
