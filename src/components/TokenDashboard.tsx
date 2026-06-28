import styles from './TokenDashboard.module.css';
import { useSessionStore } from '../stores/sessionStore';
import { useT } from '../i18n/useT';

function formatTokens(tokens: number): string {
  if (tokens >= 1_000_000) return `${(tokens / 1_000_000).toFixed(1)}M`;
  if (tokens >= 1_000) return `${(tokens / 1_000).toFixed(1)}K`;
  return tokens.toString();
}

function formatCost(cost: number): string {
  return `$${cost.toFixed(4)}`;
}

export function TokenDashboard() {
  const t = useT();
  const sessions = useSessionStore((s) => s.sessions);
  const currentSessionId = useSessionStore((s) => s.currentSessionId);

  const currentSession = sessions.find((s) => s.id === currentSessionId);
  const currentTokens = currentSession?.tokens ?? {
    promptTokens: 0,
    completionTokens: 0,
    totalTokens: 0,
    cost: 0,
  };

  const totalTokens = sessions.reduce((sum, s) => sum + s.tokens.totalTokens, 0);
  const totalCost = sessions.reduce((sum, s) => sum + s.tokens.cost, 0);
  const totalPrompt = sessions.reduce((sum, s) => sum + s.tokens.promptTokens, 0);
  const totalCompletion = sessions.reduce((sum, s) => sum + s.tokens.completionTokens, 0);

  const promptRatio =
    currentTokens.totalTokens > 0
      ? (currentTokens.promptTokens / currentTokens.totalTokens) * 100
      : 0;

  return (
    <div className={styles.dashboard}>
      <div className={styles.section}>
        <h3 className={styles.title}>{t('tokens.current')}</h3>
        <div className={styles.stats}>
          <div className={styles.stat}>
            <span className={styles.label}>{t('tokens.input')}</span>
            <span className={styles.value}>{formatTokens(currentTokens.promptTokens)}</span>
          </div>
          <div className={styles.stat}>
            <span className={styles.label}>{t('tokens.output')}</span>
            <span className={styles.value}>{formatTokens(currentTokens.completionTokens)}</span>
          </div>
          <div className={styles.stat}>
            <span className={styles.label}>{t('tokens.total')}</span>
            <span className={styles.value}>{formatTokens(currentTokens.totalTokens)}</span>
          </div>
          <div className={styles.stat}>
            <span className={styles.label}>{t('tokens.cost')}</span>
            <span className={styles.value}>{formatCost(currentTokens.cost)}</span>
          </div>
        </div>
        {currentTokens.totalTokens > 0 && (
          <div className={styles.ratio}>
            <div className={styles.ratioBar}>
              <div
                className={styles.ratioPrompt}
                style={{ width: `${promptRatio}%` }}
              />
            </div>
            <span className={styles.ratioLabel}>
              {t('tokens.input')} {promptRatio.toFixed(0)}% / {t('tokens.output')} {(100 - promptRatio).toFixed(0)}%
            </span>
          </div>
        )}
      </div>

      {sessions.length > 1 && (
        <div className={styles.section}>
          <h3 className={styles.title}>{t('tokens.allSessions', { n: sessions.length })}</h3>
          <div className={styles.stats}>
            <div className={styles.stat}>
              <span className={styles.label}>{t('tokens.input')}</span>
              <span className={styles.value}>{formatTokens(totalPrompt)}</span>
            </div>
            <div className={styles.stat}>
              <span className={styles.label}>{t('tokens.output')}</span>
              <span className={styles.value}>{formatTokens(totalCompletion)}</span>
            </div>
            <div className={styles.stat}>
              <span className={styles.label}>{t('tokens.total')}</span>
              <span className={styles.value}>{formatTokens(totalTokens)}</span>
            </div>
            <div className={styles.stat}>
              <span className={styles.label}>{t('tokens.cost')}</span>
              <span className={styles.value}>{formatCost(totalCost)}</span>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
