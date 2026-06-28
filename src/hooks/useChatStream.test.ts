// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

let lastChannel: { onmessage: ((e: unknown) => void) | null } | null = null;

vi.mock('@tauri-apps/api/core', () => ({
  Channel: class {
    onmessage: ((e: unknown) => void) | null = null;
    constructor() { lastChannel = this; }
  },
  invoke: vi.fn(() => new Promise(() => {})),
}));

import { renderHook } from '@testing-library/react';
import { useChatStream, STREAM_INACTIVITY_MS } from './useChatStream';
import type { StreamEvent } from '../types/stream';

describe('useChatStream watchdog', () => {
  beforeEach(() => { vi.useFakeTimers(); lastChannel = null; });
  afterEach(() => { vi.useRealTimers(); vi.restoreAllMocks(); });

  it('aborts with STREAM_TIMEOUT after inactivity, but not while events flow', async () => {
    const { result } = renderHook(() => useChatStream());
    const gen = result.current.streamChat({
      sessionId: 's', messages: [], providerId: 'p', model: 'm',
    } as never);

    const events: StreamEvent[] = [];
    const consume = (async () => { for await (const e of gen) events.push(e); })();

    await vi.advanceTimersByTimeAsync(0);
    expect(lastChannel).not.toBeNull();

    await vi.advanceTimersByTimeAsync(STREAM_INACTIVITY_MS - 1000);
    lastChannel!.onmessage!({ type: 'content_delta', content: 'hi' });
    await vi.advanceTimersByTimeAsync(0);
    expect(events.some((e) => e.type === 'content_delta')).toBe(true);
    expect(events.some((e) => e.type === 'error')).toBe(false);

    await vi.advanceTimersByTimeAsync(STREAM_INACTIVITY_MS + 100);
    await consume;

    const err = events.find((e) => e.type === 'error') as { type: 'error'; code: string } | undefined;
    expect(err?.code).toBe('STREAM_TIMEOUT');
  });
});
