import { describe, it, expect, vi, beforeEach } from 'vitest';

const typedInvoke = vi.fn();
vi.mock('../utils/ipc', () => ({
  typedInvoke: (cmd: string, args?: Record<string, unknown>) => typedInvoke(cmd, args),
  normalizeError: (e: unknown) => ({
    code: 'internal',
    message: e instanceof Error ? e.message : typeof e === 'object' && e && 'message' in e
      ? String((e as { message: unknown }).message)
      : String(e),
    retryable: false,
    retryAfter: null,
  }),
}));

import { useMcpStore } from './mcpStore';

beforeEach(() => {
  typedInvoke.mockReset();
  useMcpStore.setState({ servers: [], statuses: [], tools: [], loading: false, error: null });
});

describe('mcpStore', () => {
  it('load aggregates config, status, tools', async () => {
    typedInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'mcp_config_list')
        return Promise.resolve(JSON.stringify([{ name: 'fs', command: 'npx', args: [], env: {}, disabled: false }]));
      if (cmd === 'tool_mcp_status')
        return Promise.resolve(JSON.stringify([{ name: 'fs', connected: true, toolCount: 1, disabled: false }]));
      if (cmd === 'tool_mcp_list_tools')
        return Promise.resolve(JSON.stringify([{ name: 'mcp__fs__read', description: 'read', parameters: {} }]));
      return Promise.resolve('[]');
    });
    await useMcpStore.getState().load();
    const s = useMcpStore.getState();
    expect(s.servers).toHaveLength(1);
    expect(s.statuses[0].connected).toBe(true);
    expect(s.toolsForServer('fs')).toHaveLength(1);
    expect(s.toolsForServer('other')).toHaveLength(0);
  });

  it('addServer calls command then reloads', async () => {
    typedInvoke.mockResolvedValue('[]');
    await useMcpStore.getState().addServer('fs', 'npx', ['-y'], {});
    expect(typedInvoke).toHaveBeenCalledWith('mcp_config_add', { name: 'fs', command: 'npx', args: ['-y'], env: {} });
    expect(typedInvoke).toHaveBeenCalledWith('mcp_config_list', undefined);
  });

  it('captures errors via normalizeError', async () => {
    typedInvoke.mockRejectedValue({ code: 'internal', message: 'boom' });
    await useMcpStore.getState().load();
    expect(useMcpStore.getState().error).toBe('boom');
  });
});
