// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';

const state = {
  servers: [{ name: 'fs', command: 'npx', args: ['-y'], env: {}, disabled: false }],
  statuses: [{ name: 'fs', connected: true, toolCount: 2, disabled: false }],
  tools: [{ name: 'mcp__fs__read', description: 'read a file', parameters: {} }],
  loading: false,
  error: null as string | null,
  load: vi.fn(),
  addServer: vi.fn().mockResolvedValue(undefined),
  removeServer: vi.fn(),
  setDisabled: vi.fn(),
  toolsForServer: (name: string) =>
    state.tools.filter((t) => t.name.startsWith(`mcp__${name}__`)),
};

let mockLanguage = 'en';
vi.mock('../../stores/mcpStore', () => ({ useMcpStore: () => state }));
vi.mock('../../stores/settingsStore', () => ({
  useSettingsStore: (selector: (s: { language: string }) => unknown) =>
    selector({ language: mockLanguage }),
}));

import { McpServersSection } from './McpServersSection';

beforeEach(() => {
  state.addServer.mockClear();
  state.removeServer.mockClear();
  mockLanguage = 'en';
});

afterEach(() => {
  mockLanguage = 'zh';
});

describe('McpServersSection', () => {
  it('renders configured servers with command', () => {
    render(<McpServersSection />);
    expect(screen.getByText('fs')).toBeTruthy();
    expect(screen.getByText('npx')).toBeTruthy();
  });

  it('submits the add form', () => {
    mockLanguage = 'en';
    render(<McpServersSection />);
    fireEvent.change(screen.getByPlaceholderText('Name'), { target: { value: 'gh' } });
    fireEvent.change(screen.getByPlaceholderText('Command'), { target: { value: 'docker' } });
    fireEvent.click(screen.getByText('Add'));
    expect(state.addServer).toHaveBeenCalledWith('gh', 'docker', [], {});
  });

  it('flips language between en and zh', () => {
    // Test with empty state to show the "no servers" message
    const originalServers = state.servers;
    state.servers = [];

    mockLanguage = 'en';
    const { unmount } = render(<McpServersSection />);
    expect(screen.getByText('MCP Servers')).toBeTruthy();
    expect(screen.getByText('No MCP servers configured yet.')).toBeTruthy();
    unmount();

    mockLanguage = 'zh';
    render(<McpServersSection />);
    expect(screen.getByText('MCP 服务器')).toBeTruthy();
    expect(screen.getByText('尚未配置 MCP 服务器。')).toBeTruthy();

    // Restore original state
    state.servers = originalServers;
  });
});
