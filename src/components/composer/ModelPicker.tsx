import shared from './composer.module.css';
import styles from './ModelPicker.module.css';
import { useT } from '../../i18n/useT';

/** Model selection popup (shows first 6, with an expandable "more" submenu). */
export function ModelPicker({
  models,
  currentModel,
  onSelect,
  onClose,
  moreOpen,
  onToggleMore,
}: {
  models: string[];
  currentModel: string;
  onSelect: (model: string) => void;
  onClose: () => void;
  moreOpen: boolean;
  onToggleMore: () => void;
}) {
  const t = useT();
  return (
    <div className={`${shared.popup} ${shared.popupRight} ${styles.popupModel}`}>
      {models.slice(0, 6).map((m) => (
        <button key={m} type="button"
          className={`${shared.popupItem}${m === currentModel ? ' ' + shared['popupItem--active'] : ''}`}
          onClick={() => { onSelect(m); onClose(); }}>
          {m}
        </button>
      ))}
      {models.length > 6 && (
        <>
          <div className={shared.popupDivider} />
          <button type="button" className={shared.popupItem} onClick={onToggleMore}>
            {t('models.more', { n: models.length - 6 })}
            <span className={shared.popupHint}>{moreOpen ? '▾' : '▸'}</span>
          </button>
          {moreOpen && (
            <div className={shared.popupSubmenu}>
              {models.slice(6).map((m) => (
                <button key={m} type="button"
                  className={`${shared.popupItem}${m === currentModel ? ' ' + shared['popupItem--active'] : ''}`}
                  onClick={() => { onSelect(m); onClose(); }}>
                  {m}
                </button>
              ))}
            </div>
          )}
        </>
      )}
    </div>
  );
}
