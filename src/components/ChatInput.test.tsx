// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen } from '@testing-library/react';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(() => Promise.resolve('')),
  Channel: class { onmessage: ((e: unknown) => void) | null = null; },
}));

import { ChatInput } from './ChatInput';
import { useSettingsStore } from '../stores/settingsStore';

function baseProps(overrides: Record<string, unknown> = {}) {
  return {
    value: 'hello',
    onChange: vi.fn(),
    onSend: vi.fn(),
    isStreaming: false,
    models: ['gpt-4o'],
    selectedModel: 'gpt-4o',
    onModelChange: vi.fn(),
    ...overrides,
  };
}

describe('ChatInput offline gate', () => {
  beforeEach(() => vi.clearAllMocks());

  it('disables send and shows the offline banner when offline', () => {
    render(<ChatInput {...(baseProps({ isOffline: true }) as Parameters<typeof ChatInput>[0])} />);
    const sendBtn = screen.getByLabelText('Send message') as HTMLButtonElement;
    expect(sendBtn.disabled).toBe(true);
    expect(screen.getByText(/离线/)).toBeTruthy();
  });

  it('enables send and hides the offline banner when online', () => {
    render(<ChatInput {...(baseProps({ isOffline: false }) as Parameters<typeof ChatInput>[0])} />);
    const sendBtn = screen.getByLabelText('Send message') as HTMLButtonElement;
    expect(sendBtn.disabled).toBe(false);
    expect(screen.queryByText(/离线/)).toBeNull();
  });
});

describe('ChatInput i18n wiring', () => {
  beforeEach(() => vi.clearAllMocks());
  afterEach(() => useSettingsStore.setState({ language: 'zh' }));

  it('renders the offline banner in English when language is en', () => {
    useSettingsStore.setState({ language: 'en' });
    render(<ChatInput {...(baseProps({ isOffline: true }) as Parameters<typeof ChatInput>[0])} />);
    expect(screen.getByText(/Offline — cannot send/)).toBeTruthy();
    expect(screen.queryByText(/离线/)).toBeNull();
  });

  it('renders the offline banner in Chinese when language is zh', () => {
    useSettingsStore.setState({ language: 'zh' });
    render(<ChatInput {...(baseProps({ isOffline: true }) as Parameters<typeof ChatInput>[0])} />);
    expect(screen.getByText(/离线/)).toBeTruthy();
  });
});
