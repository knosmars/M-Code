import { useEffect } from 'react';
import { useToastStore, type ToastSeverity } from '../stores/toastStore';
import styles from './Toaster.module.css';

const TOAST_DISMISS_MS = 6000;

const SEVERITY_ICON: Record<ToastSeverity, string> = {
  error: '⛔',
  warn: '⚠',
  info: 'ℹ',
};

interface ToastItemProps {
  id: string;
  severity: ToastSeverity;
  message: string;
  onDismiss: (id: string) => void;
}

function ToastItem({ id, severity, message, onDismiss }: ToastItemProps) {
  useEffect(() => {
    const timer = setTimeout(() => onDismiss(id), TOAST_DISMISS_MS);
    return () => clearTimeout(timer);
  }, [id, onDismiss]);

  return (
    <div className={`${styles.toast} ${styles[`toast--${severity}`]}`}>
      <span className={styles.icon}>{SEVERITY_ICON[severity]}</span>
      <span className={styles.message}>{message}</span>
      <button className={styles.dismiss} onClick={() => onDismiss(id)} aria-label="Dismiss">
        ×
      </button>
    </div>
  );
}

export function Toaster() {
  const toasts = useToastStore((s) => s.toasts);
  const dismissToast = useToastStore((s) => s.dismissToast);

  if (toasts.length === 0) return null;

  return (
    <div className={styles.toasts}>
      {toasts.map((t) => (
        <ToastItem
          key={t.id}
          id={t.id}
          severity={t.severity}
          message={t.message}
          onDismiss={dismissToast}
        />
      ))}
    </div>
  );
}
