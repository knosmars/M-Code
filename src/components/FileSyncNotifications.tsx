import { useEffect } from 'react';
import { useFileSyncStore } from '../stores/fileSyncStore';
import styles from './FileSyncNotifications.module.css';

const AUTO_DISMISS_MS = 8000;

interface NotificationItemProps {
  id: string;
  message: string;
  onDismiss: (id: string) => void;
}

function NotificationItem({ id, message, onDismiss }: NotificationItemProps) {
  useEffect(() => {
    const timer = setTimeout(() => onDismiss(id), AUTO_DISMISS_MS);
    return () => clearTimeout(timer);
  }, [id, onDismiss]);

  return (
    <div className={styles.item}>
      <span className={styles.icon}>⚠</span>
      <span className={styles.message}>{message}</span>
      <button
        className={styles.dismiss}
        onClick={() => onDismiss(id)}
        aria-label="Dismiss"
      >
        ×
      </button>
    </div>
  );
}

export function FileSyncNotifications() {
  const notifications = useFileSyncStore((s) => s.notifications);
  const dismissNotification = useFileSyncStore((s) => s.dismissNotification);

  if (notifications.length === 0) return null;

  return (
    <div className={styles.list}>
      {notifications.map((n) => (
        <NotificationItem
          key={n.id}
          id={n.id}
          message={n.message}
          onDismiss={dismissNotification}
        />
      ))}
    </div>
  );
}
