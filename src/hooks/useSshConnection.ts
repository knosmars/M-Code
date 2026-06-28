import { useState, useEffect, useCallback } from 'react';
import { typedInvoke } from '../utils/ipc';
import { useT } from '../i18n/useT';

const SSH_CRED_KEY = 'meyatu-ssh-creds';

export interface SshForm {
  host: string;
  port: string;
  username: string;
  password: string;
  keyPath: string;
}

interface SshExecResult {
  success: boolean;
  stdout: string;
  stderr: string;
  message: string;
}

/**
 * Owns all SSH connection state for the composer's SSH menu: the connection
 * form, persisted credentials, connect/disconnect, and remote command runs.
 *
 * Extracted from ChatInput to shrink that component; the JSX consumes the
 * returned values under their original names so the markup is unchanged.
 *
 * `runSshCommand` takes an optional `onDone` callback (the caller passes its
 * menu-close handler) so the hook stays free of menu-coordination concerns.
 */
export function useSshConnection(onSend: (text: string) => void) {
  const t = useT();
  const [sshMoreOpen, setSshMoreOpen] = useState(false);
  const [sshConnected, setSshConnected] = useState(false);
  const [sshHost, setSshHost] = useState('');
  const [sshConnectOpen, setSshConnectOpen] = useState(false);
  const [sshForm, setSshForm] = useState<SshForm>({ host: '', port: '22', username: 'root', password: '', keyPath: '' });
  const [sshAuthMode, setSshAuthMode] = useState<'password' | 'key'>('password');
  const [sshConnecting, setSshConnecting] = useState(false);
  const [sshError, setSshError] = useState('');

  const loadSshCreds = useCallback(() => {
    try {
      const raw = localStorage.getItem(SSH_CRED_KEY);
      if (raw) {
        const saved = JSON.parse(raw) as { host: string; port: string; username: string; password: string; keyPath?: string; authMode?: string };
        setSshForm({ host: saved.host, port: saved.port, username: saved.username, password: saved.password, keyPath: saved.keyPath || '' });
        if (saved.authMode === 'key') setSshAuthMode('key');
        setSshHost(saved.host);
        setSshConnected(true);
      }
    } catch { /* ignore */ }
  }, []);

  useEffect(() => {
    loadSshCreds();
  }, [loadSshCreds]);

  const saveSshCreds = useCallback((creds: { host: string; port: string; username: string; password: string; keyPath: string; authMode: string }) => {
    try { localStorage.setItem(SSH_CRED_KEY, JSON.stringify(creds)); } catch { /* ignore */ }
  }, []);

  const runSshCommand = useCallback(async (label: string, command: string, onDone?: () => void) => {
    if (!sshForm.host.trim()) {
      onSend('SSH 未连接。请先在 SSH 菜单顶部填写连接信息并点击连接。');
      onDone?.();
      return;
    }
    try {
      const result = await typedInvoke<SshExecResult>('tool_ssh_exec', {
        host: sshForm.host.trim(),
        port: Number(sshForm.port) || 22,
        username: sshForm.username.trim() || 'root',
        password: sshAuthMode === 'password' ? (sshForm.password || undefined) : undefined,
        key_path: sshAuthMode === 'key' ? sshForm.keyPath.trim() || undefined : undefined,
        command,
      });
      const output = result.success ? result.stdout : (result.stderr || result.message);
      onSend(`SSH ${label} (${sshForm.username}@${sshForm.host}):\n\`\`\`\n${output}\n\`\`\`\n\n请分析以上结果。`);
    } catch (e) {
      onSend(`SSH ${label} 失败: ${String(e)}`);
    }
    onDone?.();
  }, [sshForm, sshAuthMode, onSend]);

  const handleSshConnect = useCallback(async () => {
    if (!sshForm.host.trim()) return;
    if (sshAuthMode === 'key' && !sshForm.keyPath.trim()) {
      setSshError(t('ssh.error.noKeyFile'));
      return;
    }
    setSshConnecting(true);
    setSshError('');
    try {
      const result = await typedInvoke<SshExecResult>('tool_ssh_exec', {
        host: sshForm.host.trim(),
        port: Number(sshForm.port) || 22,
        username: sshForm.username.trim() || 'root',
        password: sshAuthMode === 'password' ? (sshForm.password || undefined) : undefined,
        key_path: sshAuthMode === 'key' ? sshForm.keyPath.trim() : undefined,
        command: 'hostname',
      });
      if (result.success) {
        setSshConnected(true);
        setSshHost(sshForm.host.trim());
        setSshConnectOpen(false);
        setSshError('');
        saveSshCreds({ ...sshForm, authMode: sshAuthMode });
      } else {
        setSshError(result.stderr || result.message || t('ssh.error.connectFailed'));
      }
    } catch (e) {
      setSshConnected(false);
      setSshError(String(e));
    } finally {
      setSshConnecting(false);
    }
  }, [t, sshForm, sshAuthMode, saveSshCreds]);

  const handleSshDisconnect = useCallback(() => {
    setSshConnected(false);
    setSshHost('');
    try { localStorage.removeItem(SSH_CRED_KEY); } catch { /* ignore */ }
  }, []);

  return {
    sshMoreOpen, setSshMoreOpen,
    sshConnected, sshHost,
    sshConnectOpen, setSshConnectOpen,
    sshForm, setSshForm,
    sshAuthMode, setSshAuthMode,
    sshConnecting,
    sshError, setSshError,
    handleSshConnect, handleSshDisconnect, runSshCommand,
  };
}
