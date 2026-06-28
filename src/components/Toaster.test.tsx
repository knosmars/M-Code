// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, fireEvent, act } from '@testing-library/react';
import { Toaster } from './Toaster';
import { useToastStore } from '../stores/toastStore';

beforeEach(() => {
  useToastStore.setState({ toasts: [] });
  vi.useFakeTimers();
});
afterEach(() => {
  vi.runOnlyPendingTimers();
  vi.useRealTimers();
});

describe('Toaster', () => {
  it('renders nothing when there are no toasts', () => {
    const { container } = render(<Toaster />);
    expect(container.firstChild).toBeNull();
  });

  it('renders each toast message', () => {
    act(() => {
      useToastStore.getState().addToast('error', 'save failed');
      useToastStore.getState().addToast('warn', 'index failed');
    });
    render(<Toaster />);
    expect(screen.getByText('save failed')).toBeTruthy();
    expect(screen.getByText('index failed')).toBeTruthy();
  });

  it('auto-dismisses a toast after the timeout', () => {
    act(() => {
      useToastStore.getState().addToast('error', 'gone soon');
    });
    render(<Toaster />);
    expect(screen.getByText('gone soon')).toBeTruthy();
    act(() => {
      vi.advanceTimersByTime(6000);
    });
    expect(screen.queryByText('gone soon')).toBeNull();
    expect(useToastStore.getState().toasts.length).toBe(0);
  });

  it('dismisses a toast when the dismiss button is clicked', () => {
    act(() => {
      useToastStore.getState().addToast('error', 'click me away');
    });
    render(<Toaster />);
    fireEvent.click(screen.getByLabelText('Dismiss'));
    expect(screen.queryByText('click me away')).toBeNull();
  });

  it('clears the timer on unmount (no dangling dismiss)', () => {
    act(() => {
      useToastStore.getState().addToast('error', 'unmount me');
    });
    const { unmount } = render(<Toaster />);
    unmount();
    // Advancing time after unmount must not throw or mutate state.
    act(() => {
      vi.advanceTimersByTime(6000);
    });
    expect(useToastStore.getState().toasts.length).toBe(1);
  });
});
