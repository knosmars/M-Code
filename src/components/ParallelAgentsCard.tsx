import { useT } from '../i18n/useT';
import type { ParallelAgentState } from '../agent/parallelEvents';
import styles from './ParallelAgentsCard.module.css';

const GLYPH: Record<ParallelAgentState['status'], string> = {
  running: '●',
  done: '✓',
  error: '✗',
};

export function ParallelAgentsCard({ agents }: { agents: ParallelAgentState[] }) {
  const t = useT();
  if (agents.length === 0) return null;
  const doneCount = agents.filter((a) => a.status !== 'running').length;

  return (
    <div className={styles.card} role="group" aria-label="Parallel agents">
      <div className={styles.header}>
        <span>⛓ {t('agents.cardTitle', { n: agents.length })}</span>
        <span className={styles.count}>{t('agents.cardCount', { done: doneCount, total: agents.length })}</span>
      </div>
      <ul className={styles.lanes}>
        {agents.map((a) => (
          <li key={a.taskId} className={`${styles.lane} ${styles[`lane--${a.status}`]}`}>
            <span className={styles.glyph} aria-label={a.status}>{GLYPH[a.status]}</span>
            <span className={styles.name}>{a.agentName}</span>
            <span className={styles.detail}>
              {a.status === 'running' && (a.currentTool ? `${a.currentTool} · iter ${a.iterations}` : `iter ${a.iterations}`)}
              {a.status === 'done' && (a.resultSummary ? a.resultSummary : `done · ${a.iterations} iters`)}
              {a.status === 'error' && (a.error ?? 'error')}
            </span>
          </li>
        ))}
      </ul>
    </div>
  );
}
