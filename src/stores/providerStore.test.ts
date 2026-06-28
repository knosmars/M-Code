import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../utils/ipc', () => ({
  typedInvoke: vi.fn(),
  normalizeError: (e: unknown) => ({
    code: 'internal',
    message: e instanceof Error ? e.message : String(e),
    retryable: false,
    retryAfter: null,
  }),
}));

import { typedInvoke } from '../utils/ipc';
import { isProviderUsable, useProviderStore } from './providerStore';
import type { ProviderConfig } from '../types/provider';

const mk = (over: Partial<ProviderConfig>): ProviderConfig => ({
  id: 'x',
  name: 'X',
  baseUrl: 'http://x',
  models: ['m'],
  requiresApiKey: true,
  ...over,
});

describe('isProviderUsable', () => {
  it('keyless provider usable without a key', () => {
    expect(isProviderUsable(mk({ id: 'ollama', requiresApiKey: false }), new Set())).toBe(true);
  });
  it('key provider unusable when not configured', () => {
    expect(isProviderUsable(mk({ id: 'openai' }), new Set())).toBe(false);
  });
  it('key provider usable when configured', () => {
    expect(isProviderUsable(mk({ id: 'openai' }), new Set(['openai']))).toBe(true);
  });
});

const mockInvoke = vi.mocked(typedInvoke);

describe('fetchModels keyless', () => {
  beforeEach(() => {
    mockInvoke.mockReset();
    useProviderStore.setState({
      providers: [
        { id: 'ollama', name: 'Ollama', baseUrl: 'http://localhost:11434/v1', models: ['old'], requiresApiKey: false },
      ],
      activeProviderId: null,
      configuredProviders: new Set(),
      fetchingModels: new Set(),
      fetchErrors: {},
      providerHealth: {},
    });
  });

  it('skips get_api_key and marks healthy on success', async () => {
    mockInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'list_models') return Promise.resolve(JSON.stringify({ models: ['llama3.2', 'qwen2.5-coder'] }));
      return Promise.reject(new Error('unexpected ' + cmd));
    });
    await useProviderStore.getState().fetchModels('ollama');
    const s = useProviderStore.getState();
    expect(mockInvoke).not.toHaveBeenCalledWith('get_api_key', expect.anything());
    expect(mockInvoke).toHaveBeenCalledWith('list_models', { providerId: 'ollama', baseUrl: 'http://localhost:11434/v1', apiKey: '' });
    expect(s.providers[0].models).toEqual(['llama3.2', 'qwen2.5-coder']);
    expect(s.providerHealth['ollama']).toBe('healthy');
  });

  it('marks unreachable when list_models rejects', async () => {
    mockInvoke.mockRejectedValue(new Error('Connection refused'));
    await useProviderStore.getState().fetchModels('ollama');
    const s = useProviderStore.getState();
    expect(s.providerHealth['ollama']).toBe('unreachable');
    expect(s.fetchErrors['ollama']).toContain('Connection refused');
  });

  it('probeHealth delegates to fetchModels', async () => {
    mockInvoke.mockResolvedValue(JSON.stringify({ models: ['m'] }));
    await useProviderStore.getState().probeHealth('ollama');
    expect(useProviderStore.getState().providerHealth['ollama']).toBe('healthy');
  });
});
