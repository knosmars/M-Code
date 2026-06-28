import { describe, it, expect, vi, beforeEach } from 'vitest';

const h = vi.hoisted(() => ({ invokeImpl: null as null | ((cmd: string) => Promise<unknown>) }));
vi.mock('../utils/ipc', () => ({
  typedInvoke: (cmd: string) => (h.invokeImpl ?? (async () => '[]'))(cmd),
}));

import { useSessionStore } from './sessionStore';
import { useToastStore } from './toastStore';

beforeEach(() => {
  useToastStore.setState({ toasts: [] });
  h.invokeImpl = null;
});

describe('sessionStore error visibility', () => {
  it('shows an error toast when loadSessions fails', async () => {
    h.invokeImpl = async () => { throw new Error('db down'); };
    await useSessionStore.getState().loadSessions();
    const toasts = useToastStore.getState().toasts;
    expect(toasts.some((t) => t.severity === 'error' && t.message.includes('会话加载失败'))).toBe(true);
  });

  it('shows an error toast when session persistence fails after all retries', async () => {
    vi.useFakeTimers();
    h.invokeImpl = async (cmd: string) => {
      if (cmd === 'db_save_session') throw new Error('disk full');
      return '[]';
    };
    // createSession persists fire-and-forget via persistWithRetry (3 attempts,
    // 500ms + 1000ms backoff). Advance through both backoffs with async flushing.
    useSessionStore.getState().createSession('openai', 'gpt-4o');
    await vi.advanceTimersByTimeAsync(2000);
    const toasts = useToastStore.getState().toasts;
    expect(toasts.some((t) => t.severity === 'error' && t.message.includes('会话保存失败'))).toBe(true);
    vi.useRealTimers();
  });
});
