// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook } from '@testing-library/react';

const typedInvoke = vi.fn();
vi.mock('../utils/ipc', () => ({
  typedInvoke: (...a: unknown[]) => typedInvoke(...a),
}));

import { useAutoIndex, AUTO_INDEX_INTERVAL_MS } from './useAutoIndex';

beforeEach(() => {
  typedInvoke.mockReset();
  typedInvoke.mockResolvedValue('ok');
  vi.useFakeTimers();
});
afterEach(() => {
  vi.useRealTimers();
});

describe('useAutoIndex', () => {
  it('does nothing when disabled', () => {
    renderHook(() => useAutoIndex('.', false));
    vi.advanceTimersByTime(AUTO_INDEX_INTERVAL_MS * 3);
    expect(typedInvoke).not.toHaveBeenCalled();
  });

  it('indexes once on mount when enabled', () => {
    renderHook(() => useAutoIndex('/ws', true));
    expect(typedInvoke).toHaveBeenCalledWith('tool_semantic_index', { path: '.' });
    expect(typedInvoke).toHaveBeenCalledTimes(1);
  });

  it('re-indexes on each interval tick', async () => {
    renderHook(() => useAutoIndex('/ws', true));
    expect(typedInvoke).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(AUTO_INDEX_INTERVAL_MS);
    expect(typedInvoke).toHaveBeenCalledTimes(2);
  });

  it('skips overlapping ticks while a run is in flight', async () => {
    let resolveRun!: (v: string) => void;
    typedInvoke.mockReturnValue(new Promise<string>((r) => { resolveRun = r; }));
    renderHook(() => useAutoIndex('/ws', true));
    expect(typedInvoke).toHaveBeenCalledTimes(1); // mount run, unresolved
    await vi.advanceTimersByTimeAsync(AUTO_INDEX_INTERVAL_MS);
    expect(typedInvoke).toHaveBeenCalledTimes(1); // tick skipped: in flight
    resolveRun('ok');
  });

  it('stays silent on failure and keeps trying on later ticks', async () => {
    typedInvoke.mockRejectedValue(new Error('endpoint down'));
    renderHook(() => useAutoIndex('/ws', true));
    await vi.advanceTimersByTimeAsync(0); // settle rejection
    expect(typedInvoke).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(AUTO_INDEX_INTERVAL_MS);
    expect(typedInvoke).toHaveBeenCalledTimes(2); // silent failure doesn't disable
  });

  it('clears the interval when disabled', async () => {
    const { rerender } = renderHook(({ e }) => useAutoIndex('/ws', e), {
      initialProps: { e: true },
    });
    expect(typedInvoke).toHaveBeenCalledTimes(1);
    rerender({ e: false });
    await vi.advanceTimersByTimeAsync(AUTO_INDEX_INTERVAL_MS * 2);
    expect(typedInvoke).toHaveBeenCalledTimes(1); // no new calls after disable
  });
});
