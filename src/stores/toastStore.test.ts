import { describe, it, expect, beforeEach } from 'vitest';
import { useToastStore } from './toastStore';

beforeEach(() => {
  useToastStore.setState({ toasts: [] });
});

describe('toastStore', () => {
  it('addToast appends a toast with severity, message, and a unique id', () => {
    useToastStore.getState().addToast('error', 'boom');
    useToastStore.getState().addToast('warn', 'careful');
    const toasts = useToastStore.getState().toasts;
    expect(toasts.length).toBe(2);
    expect(toasts[0]).toMatchObject({ severity: 'error', message: 'boom' });
    expect(toasts[1]).toMatchObject({ severity: 'warn', message: 'careful' });
    expect(toasts[0].id).not.toBe(toasts[1].id);
  });

  it('dismissToast removes only the matching toast', () => {
    const store = useToastStore.getState();
    store.addToast('error', 'a');
    store.addToast('info', 'b');
    const [first, second] = useToastStore.getState().toasts;
    useToastStore.getState().dismissToast(first.id);
    const remaining = useToastStore.getState().toasts;
    expect(remaining.length).toBe(1);
    expect(remaining[0].id).toBe(second.id);
  });
});
