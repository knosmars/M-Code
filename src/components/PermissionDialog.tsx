import type { ToolCall } from '../types/message';
import type { PermissionDecision } from '../agent/tools';
import styles from './PermissionDialog.module.css';

interface PermissionDialogProps {
  /** The tool call requesting permission. */
  toolCall: ToolCall;
  /** Called with the user's decision. */
  onDecide: (decision: PermissionDecision) => void;
}

/**
 * Permission dialog shown when the agent wants to execute a write/side-effect tool.
 *
 * Displays the tool name, arguments, and approve/deny/always-allow buttons.
 */
export function PermissionDialog({
  toolCall,
  onDecide,
}: PermissionDialogProps) {
  return (
    <div className={styles.overlay} role="dialog" aria-modal="true" aria-label="Permission request">
      <div className={styles.dialog}>
        <h2 className={styles.title}>Tool Permission</h2>
        <p className={styles.desc}>
          Meyatu wants to run <strong>{toolCall.name}</strong>. This tool may modify files or execute commands.
        </p>

        <div className={styles.list}>
          <div className={styles.item}>
            <div className={styles.itemHeader}>
              <span className={styles.itemName}>{toolCall.name}</span>
            </div>
            <pre className={styles.itemArgs}>{toolCall.arguments}</pre>
          </div>
        </div>

        <div className={styles.actions}>
          <button
            type="button"
            className={`${styles.btn} ${styles.btnDeny}`}
            onClick={() => onDecide('deny')}
          >
            Deny
          </button>
          <button
            type="button"
            className={`${styles.btn} ${styles.btnApprove}`}
            onClick={() => onDecide('allow')}
          >
            Approve Once
          </button>
          <button
            type="button"
            className={`${styles.btn} ${styles.btnApprove}`}
            onClick={() => onDecide('always_allow')}
          >
            Always Allow
          </button>
        </div>
      </div>
    </div>
  );
}
