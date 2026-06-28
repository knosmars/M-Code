import { invoke } from '@tauri-apps/api/core';
import type { AppErrorShape } from '../types/ipc';

/**
 * Coerce any rejected IPC value into a structured {@link AppErrorShape}.
 *
 * Tauri rejects a `Result<_, AppError>` command with the serialized object
 * `{code, message, retryable, retryAfter}`. Legacy `Result<_, String>` commands
 * reject with a bare string. This normalizes both, plus anything unexpected.
 */
export function normalizeError(e: unknown): AppErrorShape {
  if (e && typeof e === 'object' && 'code' in e && 'message' in e) {
    const o = e as Record<string, unknown>;
    return {
      code: String(o.code),
      message: String(o.message),
      retryable: o.retryable === true,
      retryAfter: typeof o.retryAfter === 'number' ? o.retryAfter : null,
    };
  }
  const message = typeof e === 'string' ? e : String(e);
  return { code: 'internal', message, retryable: false, retryAfter: null };
}

/**
 * Typed wrapper around Tauri's `invoke`.
 *
 * Single choke point: every reject is normalized to {@link AppErrorShape} so
 * call sites can branch on `error.code` regardless of whether the command was
 * migrated to structured errors yet.
 */
export function typedInvoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  return invoke<T>(cmd, args).catch((e) => {
    throw normalizeError(e);
  });
}
