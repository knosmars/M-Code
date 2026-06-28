// @vitest-environment jsdom
import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(async () => null) }));

import { SshMenu } from './SshMenu';
import { useSettingsStore } from '../../stores/settingsStore';

function props(overrides: Record<string, unknown> = {}) {
  return {
    sshConnected: false,
    sshHost: '',
    handleSshDisconnect: vi.fn(),
    sshConnectOpen: true,
    setSshConnectOpen: vi.fn(),
    sshForm: { host: '', port: '22', username: '', password: '', keyPath: '' },
    setSshForm: vi.fn(),
    handleSshConnect: vi.fn(),
    sshConnecting: false,
    sshAuthMode: 'password',
    setSshAuthMode: vi.fn(),
    sshError: '',
    runSshCommand: vi.fn(),
    closeMenu: vi.fn(),
    sshMoreOpen: false,
    setSshMoreOpen: vi.fn(),
    onSend: vi.fn(),
    ...overrides,
  };
}

describe('SshMenu', () => {
  it('shows the connect form when disconnected and runs a command', () => {
    const runSshCommand = vi.fn();
    render(<SshMenu {...(props({ runSshCommand }) as Parameters<typeof SshMenu>[0])} />);
    expect(screen.getByText('未连接')).toBeTruthy();
    expect(screen.getByPlaceholderText('IP')).toBeTruthy();
    fireEvent.click(screen.getByText('连接测试'));
    expect(runSshCommand).toHaveBeenCalled();
  });

  it('renders SSH labels in English when language is en', () => {
    useSettingsStore.getState().setLanguage('en');
    const runSshCommand = vi.fn();
    render(<SshMenu {...(props({ runSshCommand }) as Parameters<typeof SshMenu>[0])} />);
    expect(screen.getByText('Not connected')).toBeTruthy();
    expect(screen.queryByText('未连接')).toBeFalsy();
    fireEvent.click(screen.getByText('Connection test'));
    expect(runSshCommand).toHaveBeenCalled();
    useSettingsStore.getState().setLanguage('zh');
  });
});
