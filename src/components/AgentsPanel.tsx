import { useT } from '../i18n/useT';
import type { ParallelRunState, ParallelAgentState } from '../agent/parallelEvents';
import styles from './AgentsPanel.module.css';

const GLYPH: Record<ParallelAgentState['status'], string> = {
  running: '●',
  done: '✓',
  error: '✗',
};

export function AgentsPanel({ run }: { run: ParallelRunState | null }) {
  const t = useT();
  if (!run || Object.keys(run.agents).length === 0) {
    return <div className={styles.empty}>{t('agents.empty')}</div>;
  }
  const agents = Object.values(run.agents);
  return (
    <div className={styles.panel}>
      <div className={styles.header}>
        {t('agents.headerProgress', { done: run.doneCount, total: run.total })}
      </div>
      <div className={styles.grid}>
        {agents.map((a) => (
          <div key={a.taskId} className={`${styles.card} ${styles[`card--${a.status}`]}`}>
            <div className={styles.name}>
              <span aria-label={a.status}>{GLYPH[a.status]}</span> {a.agentName}
            </div>
            <div className={styles.meta}>iter {a.iterations}</div>
            {a.status === 'running' && a.currentTool && (
              <div className={styles.tool}>{a.currentTool}</div>
            )}
            {a.status === 'done' && a.resultSummary && (
              <div className={styles.result}>{a.resultSummary}</div>
            )}
            {a.status === 'error' && <div className={styles.error}>{a.error}</div>}
          </div>
        ))}
      </div>
    </div>
  );
}
