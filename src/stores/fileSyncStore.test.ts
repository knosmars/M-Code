import { describe, it, expect, vi, beforeEach } from 'vitest';

vi.mock('../utils/ipc', () => ({
  typedInvoke: vi.fn(),
}));

import { useFileSyncStore } from './fileSyncStore';

beforeEach(() => {
  useFileSyncStore.setState({ interests: [], notifications: [] });
});

const dto = (message: string) => ({
  file: 'f.ts',
  action: 'modified',
  source: 'chat',
  target_system: 'editor',
  message,
});

describe('fileSyncStore notifications', () => {
  it('addNotification assigns an id and pushes synchronously (no timer)', () => {
    useFileSyncStore.getState().addNotification(dto('a'));
    const ns = useFileSyncStore.getState().notifications;
    expect(ns).toHaveLength(1);
    expect(ns[0].id).toBeTruthy();
    expect(ns[0].message).toBe('a');
  });

  it('does NOT auto-remove via a store timer', () => {
    vi.useFakeTimers();
    useFileSyncStore.getState().addNotification(dto('keep'));
    vi.advanceTimersByTime(60000);
    expect(useFileSyncStore.getState().notifications).toHaveLength(1);
    vi.useRealTimers();
  });

  it('assigns unique ids across notifications', () => {
    const s = useFileSyncStore.getState();
    s.addNotification(dto('a'));
    s.addNotification(dto('b'));
    const [a, b] = useFileSyncStore.getState().notifications;
    expect(a.id).not.toBe(b.id);
  });

  it('dismissNotification removes by id', () => {
    const s = useFileSyncStore.getState();
    s.addNotification(dto('a'));
    s.addNotification(dto('b'));
    const target = useFileSyncStore.getState().notifications[0].id;
    s.dismissNotification(target);
    const ns = useFileSyncStore.getState().notifications;
    expect(ns).toHaveLength(1);
    expect(ns[0].message).toBe('b');
  });
});
