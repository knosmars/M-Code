// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, act } from '@testing-library/react';
import { useFileSyncStore } from '../stores/fileSyncStore';
import { FileSyncNotifications } from './FileSyncNotifications';

vi.mock('../utils/ipc', () => ({ typedInvoke: vi.fn() }));

beforeEach(() => {
  useFileSyncStore.setState({ interests: [], notifications: [] });
});
afterEach(() => {
  vi.useRealTimers();
});

const dto = (message: string) => ({
  file: 'f.ts', action: 'modified', source: 'chat', target_system: 'editor', message,
});

describe('FileSyncNotifications', () => {
  it('renders notification messages', () => {
    act(() => useFileSyncStore.getState().addNotification(dto('hello')));
    render(<FileSyncNotifications />);
    expect(screen.getByText('hello')).toBeTruthy();
  });

  it('dismiss button removes the notification by id', () => {
    act(() => useFileSyncStore.getState().addNotification(dto('bye')));
    render(<FileSyncNotifications />);
    fireEvent.click(screen.getByLabelText('Dismiss'));
    expect(useFileSyncStore.getState().notifications).toHaveLength(0);
  });

  it('auto-dismisses after 8s via a component-owned timer', () => {
    vi.useFakeTimers();
    act(() => useFileSyncStore.getState().addNotification(dto('temp')));
    render(<FileSyncNotifications />);
    expect(useFileSyncStore.getState().notifications).toHaveLength(1);
    act(() => vi.advanceTimersByTime(8000));
    expect(useFileSyncStore.getState().notifications).toHaveLength(0);
  });

  it('clears the timer on unmount (no dismiss after unmount)', () => {
    vi.useFakeTimers();
    act(() => useFileSyncStore.getState().addNotification(dto('keep')));
    const { unmount } = render(<FileSyncNotifications />);
    unmount();
    act(() => vi.advanceTimersByTime(8000));
    expect(useFileSyncStore.getState().notifications).toHaveLength(1);
  });
});
