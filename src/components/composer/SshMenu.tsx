import shared from './composer.module.css';
import styles from './SshMenu.module.css';
import { typedInvoke } from '../../utils/ipc';
import { useT } from '../../i18n/useT';
import type { useSshConnection } from '../../hooks/useSshConnection';

type SshState = ReturnType<typeof useSshConnection>;

/** SSH shortcuts popup: connect/disconnect form + remote command shortcuts. */
export function SshMenu({
  sshConnected,
  sshHost,
  handleSshDisconnect,
  sshConnectOpen,
  setSshConnectOpen,
  sshForm,
  setSshForm,
  handleSshConnect,
  sshConnecting,
  sshAuthMode,
  setSshAuthMode,
  sshError,
  runSshCommand,
  closeMenu,
  sshMoreOpen,
  setSshMoreOpen,
  onSend,
}: {
  sshConnected: SshState['sshConnected'];
  sshHost: SshState['sshHost'];
  handleSshDisconnect: SshState['handleSshDisconnect'];
  sshConnectOpen: SshState['sshConnectOpen'];
  setSshConnectOpen: SshState['setSshConnectOpen'];
  sshForm: SshState['sshForm'];
  setSshForm: SshState['setSshForm'];
  handleSshConnect: SshState['handleSshConnect'];
  sshConnecting: SshState['sshConnecting'];
  sshAuthMode: SshState['sshAuthMode'];
  setSshAuthMode: SshState['setSshAuthMode'];
  sshError: SshState['sshError'];
  runSshCommand: SshState['runSshCommand'];
  closeMenu: () => void;
  sshMoreOpen: SshState['sshMoreOpen'];
  setSshMoreOpen: SshState['setSshMoreOpen'];
  onSend: (text?: string) => void;
}) {
  const t = useT();
  return (
    <div className={`${shared.popup} ${styles.popupSsh}`}>
      {sshConnected ? (
        <button
          type="button"
          className={shared.popupStatus}
          title={t('ssh.connected', { host: sshHost })}
          onClick={handleSshDisconnect}
        >
          <span className={`${shared.popupStatusDot} ${shared['popupStatusDot--online']}`} />
          <span className={shared.popupStatusRepo}>{sshHost}</span>
          <span className={shared.popupStatusUser} style={{ marginLeft: 'auto', opacity: 0.6, fontSize: '10px' }}>{t('ssh.disconnect')}</span>
        </button>
      ) : (
        <button
          type="button"
          className={shared.popupStatus}
          title={t('ssh.clickConnect')}
          onClick={() => setSshConnectOpen((v) => !v)}
        >
          <span className={`${shared.popupStatusDot} ${shared['popupStatusDot--offline']}`} />
          <span className={shared.popupStatusRepo}>{t('ssh.disconnected')}</span>
        </button>
      )}
      {sshConnectOpen && (
        <div className={styles.sshConnect}>
          <div className={styles.sshConnectRow}>
            <input type="text" className={styles.sshInput} placeholder="IP"
              value={sshForm.host} onChange={(e) => setSshForm((f) => ({ ...f, host: e.target.value }))} />
            <input type="text" className={`${styles.sshInput} ${styles['sshInput--short']}`} placeholder={t('ssh.port')}
              value={sshForm.port} onChange={(e) => setSshForm((f) => ({ ...f, port: e.target.value }))} />
            <input type="text" className={styles.sshInput} placeholder={t('ssh.username')}
              value={sshForm.username} onChange={(e) => setSshForm((f) => ({ ...f, username: e.target.value }))} />
            <button type="button" className={styles.sshConnectBtn}
              onClick={handleSshConnect} disabled={sshConnecting || !sshForm.host.trim()}>
              {sshConnecting ? '...' : t('ssh.connect')}
            </button>
          </div>
          <div className={styles.sshAuthToggle}>
            <button type="button"
              className={`${styles.sshAuthBtn} ${sshAuthMode === 'password' ? styles['sshAuthBtn--active'] : ''}`}
              onClick={() => setSshAuthMode('password')}>{t('ssh.authPassword')}</button>
            <button type="button"
              className={`${styles.sshAuthBtn} ${sshAuthMode === 'key' ? styles['sshAuthBtn--active'] : ''}`}
              onClick={() => setSshAuthMode('key')}>{t('ssh.authKey')}</button>
          </div>
          {sshAuthMode === 'password' ? (
            <div className={styles.sshConnectRow}>
              <input type="password" className={styles.sshInput} placeholder={t('ssh.passwordPlaceholder')}
                value={sshForm.password} onChange={(e) => setSshForm((f) => ({ ...f, password: e.target.value }))} />
            </div>
          ) : (
            <div className={styles.sshConnectRow}>
              <input type="text" className={styles.sshInput} placeholder={t('ssh.keyPathPlaceholder')}
                value={sshForm.keyPath} onChange={(e) => setSshForm((f) => ({ ...f, keyPath: e.target.value }))} readOnly />
              <button type="button" className={styles.sshConnectBtn}
                onClick={() => {
                  typedInvoke<string | null>('tool_pick_file').then((p) => {
                    if (p) setSshForm((f) => ({ ...f, keyPath: p }));
                  }).catch(() => {});
                }}>{t('ssh.pickFile')}</button>
            </div>
          )}
          {sshError && (
            <div className={styles.sshError}>{sshError}</div>
          )}
        </div>
      )}
      <div className={shared.popupDivider} />
      {([
        { label: t('ssh.cmd.connTest'), cmd: 'hostname', icon: '🔌' },
        { label: t('ssh.cmd.sysInfo'), cmd: 'uname -a && cat /etc/os-release 2>/dev/null', icon: '🖥️' },
        { label: t('ssh.cmd.diskUsage'), cmd: 'df -h', icon: '💿' },
        { label: t('ssh.cmd.procList'), cmd: 'ps aux --sort=-%cpu | head -20', icon: '⚙️' },
        { label: t('ssh.cmd.memUsage'), cmd: 'free -h', icon: '🧠' },
        { label: t('ssh.cmd.netStatus'), cmd: 'ss -tuln 2>/dev/null || netstat -tuln 2>/dev/null', icon: '🌐' },
        { label: t('ssh.cmd.fileList'), cmd: 'ls -la', icon: '📁' },
      ] as const).map(({ label, cmd, icon }) => (
        <button
          key={label}
          type="button"
          className={shared.popupItem}
          onClick={() => runSshCommand(label, cmd, closeMenu)}
        >
          <span className={shared.popupItemIcon}>{icon}</span>
          {label}
        </button>
      ))}
      <div className={shared.popupDivider} />
      <button
        type="button"
        className={shared.popupItem}
        onClick={() => setSshMoreOpen((v) => !v)}
      >
        More
        <span className={shared.popupHint}>{sshMoreOpen ? '▾' : '▸'}</span>
      </button>
      {sshMoreOpen && (
        <div className={shared.popupSubmenu}>
          {([
            { label: t('ssh.cmd.svcStatus'), cmd: 'systemctl list-units --type=service --state=running 2>/dev/null | head -20' },
            { label: t('ssh.cmd.userMgmt'), cmd: 'who && id' },
            { label: t('ssh.cmd.logView'), cmd: 'journalctl -n 30 --no-pager 2>/dev/null || tail -30 /var/log/syslog 2>/dev/null' },
            { label: t('ssh.cmd.envVars'), cmd: 'env | head -30' },
            { label: t('ssh.cmd.cronJobs'), cmd: 'crontab -l 2>/dev/null' },
          ] as const).map(({ label, cmd }) => (
            <button
              key={label}
              type="button"
              className={shared.popupItem}
              onClick={() => runSshCommand(label, cmd, closeMenu)}
            >
              {label}
            </button>
          ))}
          {([
            { label: t('ssh.cmd.fileTransfer'), cmd: '使用SCP将文件传输到远程服务器' },
            { label: t('ssh.cmd.portCheck'), cmd: '检查远程服务器指定端口是否开放' },
            { label: t('ssh.cmd.execCmd'), cmd: '在远程服务器执行自定义命令' },
          ] as const).map(({ label, cmd }) => (
            <button
              key={label}
              type="button"
              className={shared.popupItem}
              onClick={() => { closeMenu(); onSend(cmd); }}
            >
              {label}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
