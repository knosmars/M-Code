// @vitest-environment jsdom
import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';

vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn(async () => undefined) }));
vi.mock('../../utils/ipc', () => ({ typedInvoke: vi.fn(async () => ({ loggedIn: false })) }));

import { GitMenu } from './GitMenu';
import { useSettingsStore } from '../../stores/settingsStore';

function props(overrides: Record<string, unknown> = {}) {
  return {
    gitInfo: null,
    ghAuth: null,
    isLoadingGit: false,
    setIsLoadingGit: vi.fn(),
    refreshGitInfo: vi.fn(),
    runGitCommand: vi.fn(),
    onClose: vi.fn(),
    onSend: vi.fn(),
    moreOpen: false,
    onToggleMore: vi.fn(),
    ...overrides,
  };
}

describe('GitMenu', () => {
  it('shows the logged-out status and git command shortcuts', () => {
    render(<GitMenu {...(props() as Parameters<typeof GitMenu>[0])} />);
    expect(screen.getByText('未登录')).toBeTruthy();
    expect(screen.getByText('Status')).toBeTruthy();
  });

  it('runs a git command via runGitCommand', () => {
    const runGitCommand = vi.fn();
    render(<GitMenu {...(props({ runGitCommand }) as Parameters<typeof GitMenu>[0])} />);
    fireEvent.click(screen.getByText('Status'));
    expect(runGitCommand).toHaveBeenCalled();
  });

  it('renders Git labels in English when language is en', () => {
    useSettingsStore.getState().setLanguage('en');
    render(<GitMenu {...(props() as Parameters<typeof GitMenu>[0])} />);
    expect(screen.getByText('Not signed in')).toBeTruthy();
    expect(screen.queryByText('未登录')).toBeFalsy();
    useSettingsStore.getState().setLanguage('zh');
  });
});
