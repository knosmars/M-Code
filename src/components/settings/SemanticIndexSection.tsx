import { useEffect, useState, useCallback } from 'react';
import { typedInvoke, normalizeError } from '../../utils/ipc';
import type { SemanticStatus, SemanticConfig } from '../../types/ipc';
import { useSettingsStore } from '../../stores/settingsStore';
import { useT } from '../../i18n/useT';
import shared from './settings.module.css';

export function SemanticIndexSection() {
  const t = useT();
  const [status, setStatus] = useState<SemanticStatus | null>(null);
  const [cfg, setCfg] = useState<SemanticConfig | null>(null);
  const [savedMsg, setSavedMsg] = useState('');
  const [indexing, setIndexing] = useState(false);
  const [error, setError] = useState('');

  const autoIndex = useSettingsStore((s) => s.autoSemanticIndex);
  const setAutoIndex = useSettingsStore((s) => s.setAutoSemanticIndex);

  const loadStatus = useCallback(async () => {
    try {
      const s = await typedInvoke<SemanticStatus>('tool_semantic_status', { path: '.' });
      setStatus(s);
    } catch (e) {
      setError(normalizeError(e).message);
    }
  }, []);

  const loadConfig = useCallback(async () => {
    try {
      const c = await typedInvoke<SemanticConfig>('tool_semantic_config_get', {});
      setCfg(c);
    } catch (e) {
      setError(normalizeError(e).message);
    }
  }, []);

  useEffect(() => {
    // setStatus/setCfg run after the awaited IPC call, not synchronously — no
    // cascading render; the lint rule over-fires on async mount fetches.
    // eslint-disable-next-line react-hooks/set-state-in-effect
    void loadStatus();
    void loadConfig();
  }, [loadStatus, loadConfig]);

  const runIndex = useCallback(async () => {
    setIndexing(true);
    setError('');
    try {
      await typedInvoke<string>('tool_semantic_index', { path: '.' });
      await loadStatus();
    } catch (e) {
      setError(normalizeError(e).message);
    } finally {
      setIndexing(false);
    }
  }, [loadStatus]);

  return (
    <div className={shared.section}>
      <h2>{t('semantic.title')}</h2>
      <p className={shared.sectionDesc}>
        {t('semantic.desc')}
      </p>

      <div className={shared.row}>
        <span className={shared.rowLabel}>{t('semantic.status')}</span>
        <span>{status ? (status.indexed ? t('semantic.indexed') : t('semantic.notIndexed')) : t('semantic.loading')}</span>
      </div>
      {status?.indexed && (
        <>
          <div className={shared.row}>
            <span className={shared.rowLabel}>{t('semantic.filesChunksLabel')}</span>
            <span>{t('semantic.filesChunks', { files: status.file_count, chunks: status.chunk_count })}</span>
          </div>
          <div className={shared.row}>
            <span className={shared.rowLabel}>{t('semantic.embedModel')}</span>
            <span>{status.embed_model ?? '—'}{status.embed_dim ? ` (${status.embed_dim}d)` : ''}</span>
          </div>
        </>
      )}

      {error && <div className={shared.error}>{error}</div>}

      <div className={shared.row}>
        <span className={shared.rowLabel}>{t('semantic.autoIndex')}</span>
        <label>
          <input
            type="checkbox"
            checked={autoIndex}
            onChange={(e) => setAutoIndex(e.target.checked)}
          />
          {t('semantic.autoIndexHint')}
        </label>
      </div>

      {cfg && (
        <>
          <div className={shared.row}>
            <span className={shared.rowLabel}>{t('semantic.endpoint')}</span>
            <input
              type="text"
              value={cfg.embed_base}
              onChange={(e) => setCfg({ ...cfg, embed_base: e.target.value })}
            />
          </div>
          <div className={shared.row}>
            <span className={shared.rowLabel}>{t('semantic.modelLabel')}</span>
            <input
              type="text"
              value={cfg.embed_model}
              onChange={(e) => setCfg({ ...cfg, embed_model: e.target.value })}
            />
          </div>
          <button
            type="button"
            onClick={async () => {
              if (!cfg) return;
              setSavedMsg('');
              setError('');
              try {
                await typedInvoke<void>('tool_semantic_config_set', {
                  embedBase: cfg.embed_base,
                  embedModel: cfg.embed_model,
                });
                setSavedMsg(t('semantic.savedMsg'));
              } catch (e) {
                setError(normalizeError(e).message);
              }
            }}
          >
            {t('semantic.saveConfig')}
          </button>
          {savedMsg && <div className={shared.sectionDesc}>{savedMsg}</div>}
        </>
      )}

      <button type="button" onClick={runIndex} disabled={indexing}>
        {indexing ? t('semantic.indexing') : status?.indexed ? t('semantic.rebuild') : t('semantic.indexNow')}
      </button>
    </div>
  );
}
