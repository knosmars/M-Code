import { useState } from 'react';
import type { Message } from '../types/message';
import { getTextContent, getImageParts } from '../types/message';
import type { TokenUsage } from '../types/session';
import { StreamingMarkdownRenderer } from './StreamingMarkdownRenderer';
import { MarkdownRenderer } from './MarkdownRenderer';
import { ParallelAgentsCard } from './ParallelAgentsCard';
import { useSettingsStore } from '../stores/settingsStore';
import { useT } from '../i18n/useT';
import styles from './MessageBubble.module.css';

interface MessageBubbleProps {
  message: Message;
  isStreaming?: boolean;
  showTimestamp?: boolean;
  onCopy?: (content: string) => void;
  onRetry?: () => void;
  /** Revert this turn's file edits (shown when the message has a checkpointId). */
  onRevertCheckpoint?: () => void;
  tokenUsage?: TokenUsage;
}

/** Format a unix timestamp to a readable time string. */
function formatTime(ts: number): string {
  return new Date(ts).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
}

/** Format tokens with K/M units: <1000 → "123", <1M → "1.2K", ≥1M → "1.2M". */
function formatTokens(tokens: number): string {
  if (tokens >= 1_000_000) return `${(tokens / 1_000_000).toFixed(1)}m`;
  if (tokens >= 1_000) return `${(tokens / 1_000).toFixed(1)}k`;
  return tokens.toString();
}

/**
 * Strip stderr content appended by the Rust backend.
 * The backend appends stderr as `[stderr]\n<content>`, so we strip everything
 * from the `[stderr]` marker onward.
 */
function stripStderr(content: string): string {
  const idx = content.indexOf('\n[stderr]');
  if (idx === -1) return content;
  return content.slice(0, idx);
}

/**
 * Claude Code style compact tool call display.
 * Each tool gets a single compact line: [sparkle icon] verb + tool name.
 * Click to expand the full JSON arguments.
 */
function ToolCallLine({
  toolCall,
}: {
  toolCall: { id: string; name: string; arguments: string };
}) {
  const [open, setOpen] = useState(false);

  const verb = (name: string): string => {
    if (/read|cat|view/i.test(name)) return 'read';
    if (/write|edit|create/i.test(name)) return 'edit';
    if (/run|exec|command|bash|shell/i.test(name)) return 'run';
    if (/grep|search|glob|find|list/i.test(name)) return 'search';
    return 'use';
  };

  const label = `${verb(toolCall.name)} ${toolCall.name}`;

  // Try to show a preview of the first argument value.
  let preview = '';
  try {
    const parsed = JSON.parse(toolCall.arguments);
    const vals = Object.values(parsed);
    if (vals.length > 0) {
      const first = String(vals[0]);
      preview = first.length > 60 ? first.slice(0, 60) + '…' : first;
    }
  } catch { /* no preview */ }

  return (
    <div className={styles.toolCallLine}>
      <button
        type="button"
        className={styles.toolCallLineBtn}
        onClick={() => setOpen((p) => !p)}
        aria-expanded={open}
      >
        <span className={styles.toolCallLineLabel}>{label}</span>
        {preview && <span className={styles.toolCallLinePreview}>{preview}</span>}
        <span className={`${styles.toolCallLineChevron}${open ? ' ' + styles['toolCallLineChevron--open'] : ''}`}>›</span>
      </button>
      {open && (
        <pre className={styles.toolCallLineArgs}>{toolCall.arguments}</pre>
      )}
    </div>
  );
}

/**
 * Collapsible tool activity section rendered in the assistant message area.
 * Displays each tool call as a compact Claude Code style line.
 */
function ToolActivity({
  toolCalls,
}: {
  toolCalls: { id: string; name: string; arguments: string }[];
}) {
  return (
    <div className={styles.toolActivity}>
      {toolCalls.map((tc) => (
        <ToolCallLine key={tc.id} toolCall={tc} />
      ))}
    </div>
  );
}

/**
 * Renders a single chat message in the Claude Code desktop style:
 *
 * - `user`: right-aligned light-grey rounded bubble.
 * - `assistant`: full-width plain text preceded by the brand sparkle, no bubble;
 *   tool activity is shown as a collapsible summary line.
 * - `system`: centered, muted, italic.
 * - `tool`: indented, monospace, subtle.
 */
export function MessageBubble({
  message,
  isStreaming = false,
  showTimestamp = false,
  onCopy,
  onRetry,
  onRevertCheckpoint,
  tokenUsage,
}: MessageBubbleProps) {
  const { role, content, toolCalls, timestamp } = message;
  const textContent = getTextContent(content);
  const imageParts = getImageParts(content);
  const hasContent = textContent.trim().length > 0 || imageParts.length > 0;
  const hasTools = Array.isArray(toolCalls) && toolCalls.length > 0;

  // Apply stderr stripping for tool messages when the setting is enabled.
  const hideToolStderr = useSettingsStore((s) => s.hideToolStderr);
  const showToolCalls = useSettingsStore((s) => s.showToolCalls);
  const displayContent =
    role === 'tool' && hideToolStderr ? stripStderr(textContent) : textContent;

  const t = useT();

  // Don't render empty messages as blank bubbles. These get persisted by
  // interrupted/errored turns (e.g. a 400) — and since the assistant content
  // area has a subtle background, an empty one would show as a hollow pill.
  if (!hasContent && !hasTools && !isStreaming) {
    return null;
  }

  if (role === 'user') {
    return (
      <div className={`${styles.message} ${styles.messageUser}`}>
        <div className={styles.bubble}>
          {imageParts.length > 0 && (
            <div className={styles.images}>
              {imageParts.map((img, i) => (
                <img key={i} src={img.url} alt="User attachment" className={styles.image} />
              ))}
            </div>
          )}
          {textContent}
        </div>
        {showTimestamp && <span className={styles.timestamp}>{formatTime(timestamp)}</span>}
      </div>
    );
  }

  if (role === 'system') {
    return (
      <div className={`${styles.message} ${styles.messageSystem}`}>
        <div className={styles.body}>{displayContent}</div>
      </div>
    );
  }

  // Tool results are for AI context only — never show raw JSON to user
  if (role === 'tool') {
    return null;
  }

  // assistant — no per-message icon; the single live sparkle lives at the
  // bottom of the conversation (see ThinkingIndicator).

  return (
    <div className={`${styles.message} ${styles.messageAssistant}`}>
      <div className={styles.roleLabel}>Assistant</div>
      <div className={styles.content}>
        {showToolCalls && hasTools && <ToolActivity toolCalls={toolCalls!} />}

        {(textContent.length > 0 || isStreaming) && (
          <div className={styles.text}>
            {isStreaming ? (
              <StreamingMarkdownRenderer content={textContent} />
            ) : (
              <MarkdownRenderer content={textContent} />
            )}
          </div>
        )}

        {message.parallelSnapshot && message.parallelSnapshot.length > 0 && (
          <ParallelAgentsCard agents={message.parallelSnapshot} />
        )}

        {!isStreaming && (textContent.length > 0 || hasTools) && (
          <div className={styles.actions}>
            {onCopy && (
              <button
                type="button"
                className={styles.actionBtn}
                title="Copy"
                aria-label="Copy message"
                onClick={() => onCopy(textContent)}
              >
                <svg viewBox="0 0 16 16" width="14" height="14" fill="none" aria-hidden="true">
                  <rect x="5.5" y="0.5" width="10" height="12" rx="1.5" stroke="currentColor" />
                  <path d="M2.5 3.5h-1a1 1 0 0 0-1 1v10a1 1 0 0 0 1 1h8a1 1 0 0 0 1-1v-1" stroke="currentColor" />
                </svg>
              </button>
            )}
            {onRetry && (
              <button
                type="button"
                className={styles.actionBtn}
                title="Retry"
                aria-label="Retry"
                onClick={onRetry}
              >
                <svg viewBox="0 0 16 16" width="14" height="14" fill="none" aria-hidden="true">
                  <path d="M2 8a6 6 0 0 1 10.47-4M14 8a6 6 0 0 1-10.47 4" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
                  <path d="M14 2v4h-4M2 14v-4h4" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
                </svg>
              </button>
            )}
            {message.checkpointId && onRevertCheckpoint && (
              <button
                type="button"
                className={styles.actionBtn}
                title={t('message.rollback')}
                aria-label="Revert file changes from this turn"
                onClick={onRevertCheckpoint}
              >
                <svg viewBox="0 0 16 16" width="14" height="14" fill="none" aria-hidden="true">
                  <path d="M7 3 3 7l4 4" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
                  <path d="M3 7h6.5a4 4 0 0 1 0 8H6" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
                </svg>
              </button>
            )}
          </div>
        )}

        {tokenUsage && (
          <div className={styles.tokenFooter}>
            <span>{formatTokens(tokenUsage.promptTokens)} prompt</span>
            <span>·</span>
            <span>{formatTokens(tokenUsage.completionTokens)} completion</span>
            <span>·</span>
            <span>{formatTokens(tokenUsage.totalTokens)} total</span>
            {showTimestamp && <span className={styles.timestampInline}>{formatTime(timestamp)}</span>}
          </div>
        )}

        {tokenUsage === null && showTimestamp && <span className={styles.timestamp}>{formatTime(timestamp)}</span>}
      </div>
    </div>
  );
}
