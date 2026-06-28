import { useState, useEffect, useCallback } from 'react';
import { useProviderStore, isProviderUsable } from '../stores/providerStore';
import { useSettingsStore } from '../stores/settingsStore';
import { useViewStore } from '../stores/viewStore';
import type { ProviderConfig } from '../types/provider';
import type { Language } from '../i18n/translations';
import { useT } from '../i18n/useT';
import { McpServersSection } from './settings/McpServersSection';
import { SemanticIndexSection } from './settings/SemanticIndexSection';
import shared from './settings/settings.module.css';
import styles from './SettingsPanel.module.css';

export function SettingsPanel() {
  const t = useT();
  const goBack = useViewStore((s) => s.goBack);

  const {
    providers,
    activeProviderId,
    configuredProviders,
    initialize,
    setActiveProvider,
    addProvider,
    setApiKey,
    deleteApiKey,
    fetchModels,
    fetchingModels,
    fetchErrors,
    providerHealth,
    probeHealth,
  } = useProviderStore();

  const [apiKeys, setApiKeys] = useState<Record<string, string>>({});
  const [savingProvider, setSavingProvider] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  // ---- Custom provider form ----
  const [customName, setCustomName] = useState('');
  const [customUrl, setCustomUrl] = useState('');
  const [customKey, setCustomKey] = useState('');
  const [customError, setCustomError] = useState<string | null>(null);
  const [customNoKey, setCustomNoKey] = useState(false);

  useEffect(() => {
    initialize();
  }, [initialize]);

  useEffect(() => {
    for (const p of providers) {
      if (p.requiresApiKey === false) {
        void probeHealth(p.id);
      }
    }
    // probe once on mount; manual refresh re-probes
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const handleSaveKey = useCallback(
    async (providerId: string) => {
      const key = apiKeys[providerId]?.trim();
      if (!key) return;
      setSavingProvider(providerId);
      setError(null);
      try {
        await setApiKey(providerId, key);
        setApiKeys((prev) => {
          const next = { ...prev };
          delete next[providerId];
          return next;
        });
      } catch (e) {
        setError(`Failed to save API key: ${String(e)}`);
      } finally {
        setSavingProvider(null);
      }
    },
    [apiKeys, setApiKey]
  );

  const handleDeleteKey = useCallback(
    async (providerId: string) => {
      setError(null);
      try {
        await deleteApiKey(providerId);
      } catch (e) {
        setError(`Failed to delete API key: ${String(e)}`);
      }
    },
    [deleteApiKey]
  );

  return (
    <div className={styles.panel}>
      <div className={`${styles.header} ${styles['header--sticky']}`}>
        <button className={styles.backBtn} onClick={goBack}>
          ← Back
        </button>
        <h1>Settings</h1>
        <p>Configure your LLM providers and API keys. Keys are stored securely in your system keychain.</p>
      </div>

      {error && (
        <div className={shared.error} role="alert">
          {error}
          <button onClick={() => setError(null)}>Dismiss</button>
        </div>
      )}

      <div className={shared.section}>
        <h2>Providers</h2>

        {providers.map((provider) => {
          const isConfigured = configuredProviders.has(provider.id);
          const isActive = activeProviderId === provider.id;
          const keyValue = apiKeys[provider.id] ?? '';

          return (
            <div
              key={provider.id}
              className={`${styles.providerCard} ${isConfigured ? styles.configured : ''} ${isActive ? styles.active : ''}`}
            >
              <div className={styles.providerInfo}>
                <div className={styles.providerNameRow}>
                  <span className={styles.providerName}>{provider.name}</span>
                  {isConfigured && (
                    <span className={`${styles.providerBadge} ${styles.configuredBadge}`}>API Key Set</span>
                  )}
                  {isActive && (
                    <span className={`${styles.providerBadge} ${styles.activeBadge}`}>Active</span>
                  )}
                </div>
                <div className={styles.providerDetail}>
                  <span className={styles.providerUrl}>{provider.baseUrl}</span>
                  <span className={styles.providerModels}>
                    {fetchingModels.has(provider.id)
                      ? 'Fetching models from API...'
                      : `Models: ${provider.models.slice(0, 3).join(', ')}${provider.models.length > 3 ? ` +${provider.models.length - 3} more` : ''}`
                    }
                    {isConfigured && (
                      <button
                        className="refresh-models-btn"
                        onClick={() => fetchModels(provider.id)}
                        title="Refresh models from API"
                      >
                        ↻
                      </button>
                    )}
                  </span>
                  {fetchErrors[provider.id] && (
                    <span className={styles.fetchModelsError}>
                      {fetchErrors[provider.id]}
                    </span>
                  )}
                </div>
              </div>

              <div className={styles.providerActions}>
                {!provider.requiresApiKey && (
                  <div className="provider-health-row">
                    <span
                      className="provider-health-dot"
                      style={{
                        display: 'inline-block',
                        width: 8,
                        height: 8,
                        borderRadius: '50%',
                        background:
                          providerHealth[provider.id] === 'healthy'
                            ? '#3fb950'
                            : providerHealth[provider.id] === 'unreachable'
                              ? '#f85149'
                              : '#888',
                      }}
                    />
                    <span className={styles.hintText}>
                      {providerHealth[provider.id] === 'healthy'
                        ? t('settings.connection.connected')
                        : providerHealth[provider.id] === 'unreachable'
                          ? t('settings.connection.unreachable')
                          : t('settings.connection.unchecked')}
                    </span>
                    <button
                      className="refresh-models-btn"
                      onClick={() => probeHealth(provider.id)}
                      title={t('settings.connection.testTitle')}
                    >
                      {t('settings.connection.test')}
                    </button>
                  </div>
                )}

                {provider.requiresApiKey && (
                  <div className={styles.apiKeyInputGroup}>
                    <input
                      type="password"
                      className={styles.apiKeyInput}
                      placeholder={isConfigured ? '•••••••• (stored in keychain)' : 'Enter API key'}
                      value={keyValue}
                      onChange={(e) =>
                        setApiKeys((prev) => ({ ...prev, [provider.id]: e.target.value }))
                      }
                      onKeyDown={(e) => {
                        if (e.key === 'Enter' && keyValue.trim()) {
                          handleSaveKey(provider.id);
                        }
                      }}
                    />
                    {keyValue.trim() && (
                      <button
                        className={styles.saveKeyBtn}
                        onClick={() => handleSaveKey(provider.id)}
                        disabled={savingProvider === provider.id}
                      >
                        {savingProvider === provider.id ? 'Saving...' : 'Save'}
                      </button>
                    )}
                  </div>
                )}

                <div className={styles.providerButtons}>
                  {isProviderUsable(provider, configuredProviders) ? (
                    <button
                      className={`${styles.selectProviderBtn} ${isActive ? styles.current : ''}`}
                      onClick={() => setActiveProvider(provider.id)}
                      disabled={isActive}
                    >
                      {isActive ? 'Selected' : 'Select'}
                    </button>
                  ) : (
                    <span className={styles.hintText}>Save an API key to activate</span>
                  )}
                  {isConfigured && provider.requiresApiKey && (
                    <button
                      className={styles.deleteKeyBtn}
                      onClick={() => handleDeleteKey(provider.id)}
                    >
                      Remove Key
                    </button>
                  )}
                </div>
              </div>
            </div>
          );
        })}
      </div>

      <div className={shared.section}>
        <h2>Custom Provider</h2>
        <p className={shared.sectionDesc}>
          Add any OpenAI-compatible API endpoint. You can input the API URL, a display name, and your API key.
        </p>

        {customError && (
          <div className={shared.error} role="alert">
            {customError}
            <button onClick={() => setCustomError(null)}>Dismiss</button>
          </div>
        )}

        <div className={styles.customProviderForm}>
          <div className={styles.customFormRow}>
            <input
              type="text"
              className={styles.customInput}
              placeholder="Display name (e.g. DeepSeek)"
              value={customName}
              onChange={(e) => setCustomName(e.target.value)}
            />
            <input
              type="text"
              className={styles.customInput}
              placeholder="API URL (e.g. https://api.deepseek.com/v1)"
              value={customUrl}
              onChange={(e) => setCustomUrl(e.target.value)}
            />
          </div>
          <div className={styles.customFormRow}>
            <label className={shared.toggle} style={{ margin: '0 0 6px' }}>
              <input
                type="checkbox"
                checked={customNoKey}
                onChange={(e) => setCustomNoKey(e.target.checked)}
              />
              <span className={shared.toggleLabel}>{t('settings.localNoKey')}</span>
            </label>
          </div>
          <div className={styles.customFormRow}>
            <div className={styles.apiKeyInputGroup}>
              {!customNoKey && (
                <input
                  type="password"
                  className={styles.apiKeyInput}
                  placeholder="API key"
                  value={customKey}
                  onChange={(e) => setCustomKey(e.target.value)}
                />
              )}
              <button
                className={styles.saveKeyBtn}
                disabled={!customName.trim() || !customUrl.trim() || (!customNoKey && !customKey.trim())}
                onClick={async () => {
                  const name = customName.trim();
                  const url = customUrl.trim();
                  const key = customKey.trim();
                  if (!name || !url || (!customNoKey && !key)) {
                    setCustomError('All fields are required.');
                    return;
                  }
                  const providerId = `custom-${Date.now()}`;
                  const provider: ProviderConfig = {
                    id: providerId,
                    name,
                    baseUrl: url,
                    models: ['default'],
                    requiresApiKey: !customNoKey,
                  };
                  try {
                    addProvider(provider);
                    if (customNoKey) {
                      setActiveProvider(providerId);
                      void probeHealth(providerId);
                    } else {
                      await setApiKey(providerId, key);
                      setActiveProvider(providerId);
                    }
                    setCustomName('');
                    setCustomUrl('');
                    setCustomKey('');
                    setCustomNoKey(false);
                    setCustomError(null);
                  } catch (e) {
                    setCustomError(`Failed to add provider: ${String(e)}`);
                  }
                }}
              >
                Add Provider
              </button>
            </div>
          </div>
        </div>
      </div>

      <McpServersSection />

      <SemanticIndexSection />

      <div className={shared.section}>
        <h2>Display</h2>

        <div className={shared.row}>
          <span className={shared.rowLabel}>语言 / Language</span>
          <select
            className={styles.select}
            value={useSettingsStore((s) => s.language)}
            onChange={(e) => useSettingsStore.getState().setLanguage(e.target.value as Language)}
          >
            <option value="zh">中文</option>
            <option value="en">English</option>
          </select>
        </div>

        <label className={shared.toggle}>
          <input
            type="checkbox"
            checked={useSettingsStore((s) => s.hideToolStderr)}
            onChange={(e) => useSettingsStore.getState().setHideToolStderr(e.target.checked)}
          />
          <span className={shared.toggleLabel}>
            Hide tool stderr from messages
          </span>
          <span className={shared.toggleDesc}>
            When enabled, stderr output (e.g. Windows pwd errors, chcp warnings) is stripped from displayed tool results.
          </span>
        </label>

        <label className={shared.toggle}>
          <input
            type="checkbox"
            checked={useSettingsStore((s) => s.showToolCalls)}
            onChange={(e) => useSettingsStore.getState().setShowToolCalls(e.target.checked)}
          />
          <span className={shared.toggleLabel}>
            Show tool call activity
          </span>
          <span className={shared.toggleDesc}>
            When enabled, the list of tools executed (read, run, edit, etc.) is shown in assistant messages.
          </span>
        </label>
      </div>
    </div>
  );
}
