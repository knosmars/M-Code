import { create } from 'zustand';
import { typedInvoke, normalizeError } from '../utils/ipc';
import type { ProviderConfig } from '../types/provider';

const DEFAULT_PROVIDERS: ProviderConfig[] = [
  {
    id: 'meyatu',
    name: 'Meyatu',
    baseUrl: 'https://api.meyatu.io/v1',
    // Real gateway model ids (the previous 'meyatu-pro'/'meyatu-flash' were
    // placeholders that don't exist on the gateway -> chat returned 503
    // model_not_found). These are overwritten by fetchModels() once a key
    // is saved, but provide working defaults on first run.
    models: [
      'openai/gpt-4o',
      'deepseek-v4-pro',
      'vertex/claude-sonnet-4-6',
      'coding/gemini-2.5-pro',
    ],
    requiresApiKey: true,
  },
  {
    id: 'openai-compatible',
    name: 'OpenAI Compatible',
    baseUrl: 'https://api.openai.com/v1',
    models: ['gpt-4o', 'gpt-4o-mini', 'gpt-4-turbo', 'gpt-3.5-turbo'],
    requiresApiKey: true,
  },
  {
    id: 'anthropic',
    name: 'Anthropic',
    baseUrl: 'https://api.anthropic.com',
    models: ['claude-sonnet-4-20250514', 'claude-3-5-sonnet-20241022', 'claude-3-opus-20240229'],
    requiresApiKey: true,
  },
  {
    id: 'google',
    name: 'Google Gemini',
    baseUrl: 'https://generativelanguage.googleapis.com',
    models: ['gemini-2.5-pro', 'gemini-2.5-flash', 'gemini-2.0-flash'],
    requiresApiKey: true,
  },
  {
    // Local models via Ollama's OpenAI-compatible endpoint — no API key needed.
    // Pull models with `ollama pull <name>`; fetchModels() refreshes this list.
    id: 'ollama',
    name: 'Ollama (Local)',
    baseUrl: 'http://localhost:11434/v1',
    models: ['llama3.2', 'qwen2.5-coder', 'deepseek-r1'],
    requiresApiKey: false,
  },
];

export interface ProviderState {
  providers: ProviderConfig[];
  activeProviderId: string | null;
  selectedModel: string | null;
  initialized: boolean;
  initError: string | null;
  /** Track which providers have API keys stored in keychain */
  configuredProviders: Set<string>;
  /** Fetching models status per provider */
  fetchingModels: Set<string>;
  /** Fetch errors per provider — cleared on next successful fetch */
  fetchErrors: Record<string, string>;
  /** Initialize providers with defaults and check keychain for stored keys */
  initialize: () => Promise<void>;
  setProviders: (providers: ProviderConfig[]) => void;
  setActiveProvider: (id: string) => void;
  setSelectedModel: (model: string) => void;
  addProvider: (provider: ProviderConfig) => void;
  removeProvider: (id: string) => void;
  /** Store API key in OS keychain, then auto-fetch models */
  setApiKey: (providerId: string, apiKey: string) => Promise<void>;
  /** Check if API key exists in keychain */
  hasApiKey: (providerId: string) => Promise<boolean>;
  /** Delete API key from keychain */
  deleteApiKey: (providerId: string) => Promise<void>;
  /** Fetch available models from provider's /v1/models endpoint */
  fetchModels: (providerId: string) => Promise<void>;
  /** Health per keyless provider — probed via list_models */
  providerHealth: Record<string, ProviderHealth>;
  /** Probe a keyless provider's reachability (delegates to fetchModels) */
  probeHealth: (providerId: string) => Promise<void>;
}

async function checkKeychain(providerId: string): Promise<boolean> {
  try {
    const key = await typedInvoke<string | null>('get_api_key', { provider: providerId });
    return key !== null && key.length > 0;
  } catch (err) {
    console.error(`[providerStore] checkKeychain failed for ${providerId}:`, err);
    return false;
  }
}

export type ModelTier = 'fast' | 'balanced' | 'strong';

export type ProviderHealth = 'unknown' | 'healthy' | 'unreachable';

/** A provider is usable if it needs no key (local) or has a stored key. */
export function isProviderUsable(
  p: ProviderConfig,
  configured: Set<string>,
): boolean {
  return p.requiresApiKey === false || configured.has(p.id);
}

const FAST_PATTERNS = /flash|mini|3\.5-turbo|2\.0-flash|haiku/i;
const STRONG_PATTERNS = /opus|o1|o3|pro-preview|claude-3-opus/i;

export function classifyModelTier(model: string): ModelTier {
  if (FAST_PATTERNS.test(model)) return 'fast';
  if (STRONG_PATTERNS.test(model)) return 'strong';
  return 'balanced';
}

export function getFallbackModel(model: string, availableModels: string[]): string | null {
  const currentTier = classifyModelTier(model);
  const targetTier: ModelTier | null = currentTier === 'strong' ? 'balanced' : currentTier === 'balanced' ? 'fast' : null;
  if (!targetTier) return null;

  for (const candidate of availableModels) {
    if (candidate !== model && classifyModelTier(candidate) === targetTier) {
      return candidate;
    }
  }
  if (targetTier === 'balanced') {
    for (const candidate of availableModels) {
      if (candidate !== model && classifyModelTier(candidate) === 'fast') {
        return candidate;
      }
    }
  }
  return null;
}

export const useProviderStore = create<ProviderState>((set, get) => ({
  providers: DEFAULT_PROVIDERS,
  activeProviderId: null,
  selectedModel: DEFAULT_PROVIDERS[0].models[0],
  initialized: false,
  initError: null,
  configuredProviders: new Set(),
  fetchingModels: new Set(),
  fetchErrors: {},
  providerHealth: {},

  initialize: async () => {
    try {
      const configured = new Set<string>();
      for (const p of DEFAULT_PROVIDERS) {
        if (await checkKeychain(p.id)) {
          configured.add(p.id);
        }
      }
      const hasConfigured = configured.size > 0;
      const firstConfigured = hasConfigured ? configured.values().next().value ?? null : null;
      // Preserve a selection the user already made — initialize() may run
      // again on re-mount and must not clobber the active provider/model.
      const existingActive = get().activeProviderId;
      const nextActive = existingActive ?? firstConfigured;
      set({
        configuredProviders: configured,
        activeProviderId: nextActive,
        selectedModel: existingActive
          ? get().selectedModel
          : firstConfigured
            ? get().providers.find((p) => p.id === firstConfigured)?.models[0] ?? DEFAULT_PROVIDERS[0].models[0]
            : DEFAULT_PROVIDERS[0].models[0],
        initialized: true,
        initError: null,
      });
      for (const id of configured) {
        get().fetchModels(id).catch((err) => {
          console.error(`[providerStore] Failed to fetch models for ${id}:`, err);
        });
      }
    } catch (err) {
      set({
        initialized: true,
        initError: `Provider init failed: ${err instanceof Error ? err.message : String(err)}`,
      });
    }
  },

  setProviders: (providers) => set({ providers }),

  setActiveProvider: (id) => {
    const provider = get().providers.find((p) => p.id === id);
    set({ activeProviderId: id, selectedModel: provider?.models[0] ?? null });
  },

  setSelectedModel: (model) => set({ selectedModel: model }),

  addProvider: (provider) =>
    set((state) => ({
      providers: [...state.providers, provider],
    })),

  removeProvider: (id) =>
    set((state) => ({
      providers: state.providers.filter((p) => p.id !== id),
      activeProviderId:
        state.activeProviderId === id ? null : state.activeProviderId,
    })),

  setApiKey: async (providerId, apiKey) => {
    await typedInvoke<void>('set_api_key', { provider: providerId, apiKey });
    set((state) => {
      const configured = new Set(state.configuredProviders);
      configured.add(providerId);
      // Auto-activate this provider if none is active yet, so saving a key
      // makes it usable immediately without a separate "Select" click.
      const activeProviderId = state.activeProviderId ?? providerId;
      const selectedModel =
        state.activeProviderId === null
          ? state.providers.find((p) => p.id === providerId)?.models[0] ?? state.selectedModel
          : state.selectedModel;
      return { configuredProviders: configured, activeProviderId, selectedModel };
    });
    await get().fetchModels(providerId);
  },

  hasApiKey: async (providerId) => {
    return checkKeychain(providerId);
  },

  deleteApiKey: async (providerId) => {
    await typedInvoke<void>('delete_api_key', { provider: providerId });
    set((state) => {
      const configured = new Set(state.configuredProviders);
      configured.delete(providerId);
      return { configuredProviders: configured };
    });
  },

  fetchModels: async (providerId) => {
    const { providers, fetchingModels } = get();
    if (fetchingModels.has(providerId)) return;

    const provider = providers.find((p) => p.id === providerId);
    if (!provider) return;
    const keyless = provider.requiresApiKey === false;

    set((s) => ({
      fetchingModels: new Set(s.fetchingModels).add(providerId),
    }));

    const clearFetching = (s: ProviderState) => {
      const next = new Set(s.fetchingModels);
      next.delete(providerId);
      return next;
    };

    try {
      let key = '';
      if (!keyless) {
        const k = await typedInvoke<string | null>('get_api_key', { provider: providerId });
        if (!k) {
          set((s) => ({
            fetchErrors: { ...s.fetchErrors, [providerId]: 'No API key found in keychain. Save an API key in Settings to fetch models.' },
            fetchingModels: clearFetching(s),
          }));
          return;
        }
        key = k;
      }

      set((s) => ({
        fetchErrors: Object.fromEntries(
          Object.entries(s.fetchErrors).filter(([id]) => id !== providerId)
        ),
      }));

      const raw = await typedInvoke<string>('list_models', {
        providerId,
        baseUrl: provider.baseUrl,
        apiKey: key,
      });

      const json: { models?: string[] } = JSON.parse(raw);
      const ids = (json.models ?? []).filter(
        (id): id is string => typeof id === 'string' && id.length > 0,
      );

      if (ids.length === 0) {
        set((s) => ({
          fetchErrors: { ...s.fetchErrors, [providerId]: 'API returned no models.' },
          fetchingModels: clearFetching(s),
          ...(keyless ? { providerHealth: { ...s.providerHealth, [providerId]: 'unreachable' as ProviderHealth } } : {}),
        }));
        return;
      }

      set((s) => ({
        providers: s.providers.map((p) =>
          p.id === providerId ? { ...p, models: ids } : p,
        ),
        selectedModel: s.activeProviderId === providerId ? ids[0] : s.selectedModel,
        fetchingModels: clearFetching(s),
        fetchErrors: Object.fromEntries(
          Object.entries(s.fetchErrors).filter(([id]) => id !== providerId)
        ),
        ...(keyless ? { providerHealth: { ...s.providerHealth, [providerId]: 'healthy' as ProviderHealth } } : {}),
      }));
    } catch (e) {
      const msg = normalizeError(e).message;
      set((s) => ({
        fetchErrors: { ...s.fetchErrors, [providerId]: `Failed to fetch models: ${msg}` },
        fetchingModels: clearFetching(s),
        ...(keyless ? { providerHealth: { ...s.providerHealth, [providerId]: 'unreachable' as ProviderHealth } } : {}),
      }));
    }
  },

  probeHealth: async (providerId) => {
    await get().fetchModels(providerId);
  },
}));
