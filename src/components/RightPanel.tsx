import shared from './RightPanel.module.css';

interface Props {
  filePath: string | null;
  onClose: () => void;
  children?: React.ReactNode;
}

export function RightPanel({ filePath, onClose, children }: Props) {
  return (
    <div className={shared.rightPanel}>
      <div className={shared.rightPanelHeader}>
        <span className={shared.rightPanelFilePath} title={filePath ?? ''}>
          {filePath ?? 'No file selected'}
        </span>
        <button className={shared.rightPanelClose} onClick={onClose} title="Close panel">
          ×
        </button>
      </div>
      <div className={shared.rightPanelBody}>
        {children ?? <div className={shared.rightPanelEmpty}>Select a file to preview</div>}
      </div>
    </div>
  );
}
