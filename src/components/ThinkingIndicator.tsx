import { BrandIcon } from './BrandIcon';
import { useT } from '../i18n/useT';
import styles from './ThinkingIndicator.module.css';

interface ToolEntry {
  toolName: string;
  status: string;
}

interface ThinkingIndicatorProps {
  /** Seconds elapsed since the turn started. */
  elapsedSec: number;
  /** Tokens consumed so far this turn (0 hides the segment). */
  tokens: number;
  /** Short status phrase, e.g. "still thinking…" / "Running command…". */
  status?: string;
  /** Currently executing tools keyed by toolCallId. */
  activeTools?: Map<string, ToolEntry>;
}

/** Format elapsed seconds: <60s → "Xs", <60min → "XM Ys", <60hr → "Xh YM". */
function formatElapsed(sec: number): string {
  if (sec < 60) return `${sec}s`;
  const min = Math.floor(sec / 60);
  const remainSec = sec % 60;
  if (min < 60) return `${min}m ${remainSec}s`;
  const hr = Math.floor(min / 60);
  const remainMin = min % 60;
  return `${hr}h ${remainMin}m`;
}

/** Format tokens with K/M units: <1000 → "123", <1M → "1.2K", ≥1M → "1.2M". */
function formatTokens(tokens: number): string {
  if (tokens >= 1_000_000) return `${(tokens / 1_000_000).toFixed(1)}m`;
  if (tokens >= 1_000) return `${(tokens / 1_000).toFixed(1)}k`;
  return tokens.toString();
}

function statusIcon(status: string): string {
  if (status === 'done') return '✓';
  if (status === 'error') return '✗';
  return '⟳';
}

/**
 * The single live "working" indicator shown at the bottom of the conversation
 * while the assistant is streaming — an animated brand icon followed by a
 * status line that reveals elapsed time, token usage, and the current state.
 */
export function ThinkingIndicator({ elapsedSec, tokens, status, activeTools }: ThinkingIndicatorProps) {
  const t = useT();
  const parts: string[] = [formatElapsed(elapsedSec)];
  if (tokens > 0) parts.push(`${formatTokens(tokens)} ${t('chat.tokens')}`);
  if (status) parts.push(status);

  const runningTools = activeTools
    ? Array.from(activeTools.entries()).filter(([, v]) => v.status === 'running')
    : [];
  const doneTools = activeTools
    ? Array.from(activeTools.entries()).filter(([, v]) => v.status === 'done' || v.status === 'error')
    : [];

  return (
    <div className={styles.thinking} role="status" aria-live="polite">
      <span className={styles.icon}>
        <BrandIcon size={20} animated />
      </span>
      <div className={styles.body}>
        <span className={styles.status}>{parts.join(' · ')}</span>
        {runningTools.length > 0 && (
          <div className={styles.tools}>
            {runningTools.map(([id, { toolName }]) => (
              <span key={id} className={`${styles.tool} ${styles['tool--running']}`}>
                <span className={styles.toolIcon}>{statusIcon('running')}</span>
                {toolName}
              </span>
            ))}
            {doneTools.slice(-3).map(([id, { toolName, status: s }]) => (
              <span key={id} className={`${styles.tool} ${styles[`tool--${s}`]}`}>
                <span className={styles.toolIcon}>{statusIcon(s)}</span>
                {toolName}
              </span>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
