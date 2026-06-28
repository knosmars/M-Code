import { useState, useRef, useCallback, useEffect } from 'react';
import { Channel } from '@tauri-apps/api/core';
import { typedInvoke, normalizeError } from '../utils/ipc';
import type { ChatRequest } from '../types/ipc';
import type { StreamEvent } from '../types/stream';

/** Abort a stream that goes silent for this long (guards against a hung
 *  backend / dropped connection). Generous, to survive slow first-token. */
export const STREAM_INACTIVITY_MS = 60_000;

/**
 * A React hook that wraps Tauri 2 Channel IPC for streaming chat communication.
 *
 * Provides a `streamChat` async generator that yields `StreamEvent` values
 * as they arrive from the Rust backend, plus a `cancel` function to abort
 * an in-progress stream and an `isStreaming` reactive flag.
 */
export function useChatStream(): {
  streamChat: (request: ChatRequest) => AsyncGenerator<StreamEvent, void, unknown>;
  cancel: () => void;
  isStreaming: boolean;
} {
  const [isStreaming, setIsStreaming] = useState(false);
  const abortControllerRef = useRef<AbortController | null>(null);

  // Cancel any in-flight stream on component unmount.
  useEffect(() => {
    return () => {
      abortControllerRef.current?.abort();
    };
  }, []);

  const cancel = useCallback(() => {
    abortControllerRef.current?.abort();
  }, []);

  /**
   * Start a chat stream via Tauri 2 Channel IPC.
   *
   * Returns an async generator that yields `StreamEvent` values
   * (content_delta, tool_call, tool_result, error, done) as they
   * are received from the Rust backend through the Channel.
   *
   * The generator stops when:
   * - The Rust backend signals completion (done event)
   * - An error occurs on the IPC layer
   * - `cancel()` is called (via AbortController)
   */
  const streamChat = useCallback(async function* (
    request: ChatRequest,
  ): AsyncGenerator<StreamEvent, void, unknown> {
    const abortController = new AbortController();
    abortControllerRef.current = abortController;
    setIsStreaming(true);

    let timedOut = false;
    let watchdog: ReturnType<typeof setTimeout> | null = null;
    const armWatchdog = () => {
      if (watchdog) clearTimeout(watchdog);
      watchdog = setTimeout(() => {
        timedOut = true;
        abortController.abort();
      }, STREAM_INACTIVITY_MS);
    };
    armWatchdog();

    // Create a Tauri 2 Channel for receiving streamed events from Rust.
    const channel = new Channel<StreamEvent>();

    // Promise-based queue to bridge callback-based onmessage to async iterable.
    const queue: StreamEvent[] = [];
    let resolveNext: (() => void) | null = null;
    let invokeDone = false;
    let invokeError: unknown = null;

    channel.onmessage = (event: StreamEvent) => {
      armWatchdog();
      queue.push(event);
      resolveNext?.();
      resolveNext = null;
    };

    // Fire-and-forget the Tauri command. Events arrive via channel.onmessage
    // while the command is running. The promise resolves when the Rust handler
    // completes (stream finished or error).
    // The Rust command parameter is `on_event: Channel<StreamEvent>`, which
    // Tauri exposes to JS as the `onEvent` key. Passing `channel` makes Tauri
    // reject the call with "missing required key onEvent".
    typedInvoke<void>('stream_chat', { request, onEvent: channel })
      .then(() => {
        invokeDone = true;
      })
      .catch((error: unknown) => {
        invokeError = error;
        invokeDone = true;
      })
      .finally(() => {
        resolveNext?.();
        resolveNext = null;
      });

    const onAbort = () => {
      invokeDone = true;
      resolveNext?.();
      resolveNext = null;
    };
    abortController.signal.addEventListener('abort', onAbort, { once: true });

    try {
      // Yield events from the queue until the stream completes, errors, or
      // is cancelled.
      while (!invokeDone || queue.length > 0) {
        if (queue.length > 0) {
          const event = queue.shift();
          if (event !== undefined) {
            yield event;
          }
        } else if (!invokeDone) {
          await new Promise<void>((resolve) => {
            resolveNext = resolve;
          });
        }
      }

      // If the invoke itself failed (Tauri IPC error, not a stream error
      // event), yield a synthetic error event so the consumer can handle it.
      if (invokeError !== null) {
        const message = normalizeError(invokeError).message;
        yield { type: 'error', code: 'IPC_ERROR', message };
      }

      if (timedOut) {
        yield { type: 'error', code: 'STREAM_TIMEOUT', message: '网络超时，请检查连接' };
      }
    } finally {
      if (watchdog) clearTimeout(watchdog);
      abortController.signal.removeEventListener('abort', onAbort);
      setIsStreaming(false);
      abortControllerRef.current = null;
    }
  }, []);

  return { streamChat, cancel, isStreaming };
}
