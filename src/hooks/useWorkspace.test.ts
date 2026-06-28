// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { renderHook, waitFor, act } from '@testing-library/react';

const h = vi.hoisted(() => ({ invokeImpl: null as null | ((cmd: string) => Promise<unknown>) }));
vi.mock('../utils/ipc', () => ({
  typedInvoke: (cmd: string) => (h.invokeImpl ?? (async () => ''))(cmd),
}));

import { useWorkspace } from './useWorkspace';
import { useToastStore } from '../stores/toastStore';

beforeEach(() => {
  useToastStore.setState({ toasts: [] });
  h.invokeImpl = null;
});

describe('useWorkspace error visibility', () => {
  it('shows a warn toast when codebase indexing fails after selecting workspace', async () => {
    h.invokeImpl = async (cmd: string) => {
      if (cmd === 'tool_set_workspace') return '/tmp/test';
      if (cmd === 'tool_index_codebase') throw new Error('index boom');
      return '';
    };
    const { result } = renderHook(() => useWorkspace(vi.fn()));
    await act(async () => {
      await result.current.selectWorkspace('/tmp/test');
    });
    await waitFor(() => {
      const toasts = useToastStore.getState().toasts;
      expect(toasts.some((t) => t.severity === 'warn' && t.message.includes('代码库索引失败'))).toBe(true);
    });
  });
});
