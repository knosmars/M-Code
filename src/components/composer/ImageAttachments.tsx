import type { ImageAttachment } from '../ChatInput';
import styles from './ImageAttachments.module.css';

/** Attached-image preview strip shown above the composer textarea. */
export function ImageAttachments({
  images,
  onRemove,
}: {
  images: ImageAttachment[];
  onRemove?: (id: string) => void;
}) {
  if (images.length === 0) return null;
  return (
    <div className={styles.imageAttachments}>
      {images.map((att) => (
        <div key={att.id} className={styles.imageAttachment}>
          <img src={att.dataUrl} alt={att.name} className={styles.imageAttachmentImg} />
          {onRemove && (
            <button
              type="button"
              className={styles.imageAttachmentRemove}
              onClick={() => onRemove(att.id)}
              aria-label={`Remove ${att.name}`}
            >×</button>
          )}
        </div>
      ))}
    </div>
  );
}
