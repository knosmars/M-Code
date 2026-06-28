// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';

const store = {
  providers: [
    { id: 'ollama', name: 'Ollama (Local)', baseUrl: 'http://localhost:11434/v1', models: ['llama3.2'], requiresApiKey: false },
  ],
  activeProviderId: null as string | null,
  configuredProviders: new Set<string>(),
  fetchingModels: new Set<string>(),
  fetchErrors: {} as Record<string, string>,
  providerHealth: { ollama: 'healthy' } as Record<string, string>,
  initialize: vi.fn(),
  setActiveProvider: vi.fn(),
  addProvider: vi.fn(),
  setApiKey: vi.fn(),
  deleteApiKey: vi.fn(),
  fetchModels: vi.fn(),
  probeHealth: vi.fn(),
};
vi.mock('../stores/providerStore', async (orig) => {
  const actual = await orig<typeof import('../stores/providerStore')>();
  return { ...actual, useProviderStore: () => store };
});
vi.mock('../stores/settingsStore', () => ({
  useSettingsStore: Object.assign(() => 'zh', { getState: () => ({ setLanguage: vi.fn(), setHideToolStderr: vi.fn(), setShowToolCalls: vi.fn() }) }),
}));
vi.mock('./settings/McpServersSection', () => ({ McpServersSection: () => null }));

import { SettingsPanel } from './SettingsPanel';
import { useViewStore } from '../stores/viewStore';

beforeEach(() => {
  store.setActiveProvider.mockClear();
  store.probeHealth.mockClear();
  useViewStore.setState({ view: 'chat', previous: 'chat' });
});

describe('SettingsPanel keyless provider', () => {
  it('renders Ollama with a Select button and no API key input', () => {
    render(<SettingsPanel />);
    expect(screen.getByText('Ollama (Local)')).toBeTruthy();
    // keyless: Select button present (usable without a key)
    expect(screen.getByText('Select')).toBeTruthy();
    // keyless: no password input rendered for this card
    expect(screen.queryByPlaceholderText('Enter API key')).toBeNull();
  });

  it('probes keyless provider health on mount', () => {
    render(<SettingsPanel />);
    expect(store.probeHealth).toHaveBeenCalledWith('ollama');
  });
});

describe('SettingsPanel custom local provider', () => {
  beforeEach(() => {
    store.addProvider.mockClear();
    store.setApiKey.mockClear();
  });

  it('adds a keyless custom provider without calling setApiKey', () => {
    render(<SettingsPanel />);
    fireEvent.change(screen.getByPlaceholderText('Display name (e.g. DeepSeek)'), { target: { value: 'LM Studio' } });
    fireEvent.change(screen.getByPlaceholderText('API URL (e.g. https://api.deepseek.com/v1)'), { target: { value: 'http://localhost:1234/v1' } });
    fireEvent.click(screen.getByLabelText('本地服务（无需 API key）'));
    fireEvent.click(screen.getByText('Add Provider'));
    expect(store.addProvider).toHaveBeenCalledWith(
      expect.objectContaining({ name: 'LM Studio', baseUrl: 'http://localhost:1234/v1', requiresApiKey: false }),
    );
    expect(store.setApiKey).not.toHaveBeenCalled();
  });
});
