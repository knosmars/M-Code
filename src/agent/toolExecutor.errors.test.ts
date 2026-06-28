import { describe, it, expect } from 'vitest';
import { normalizeError } from '../utils/ipc';

// Guards the contract toolExecutor relies on: a structured AppError reject
// must yield a human-readable message, never "[object Object]".
describe('toolExecutor error normalization', () => {
  it('extracts message from a structured AppError reject', () => {
    const rejected = { code: 'not_found', message: 'File not found: x', retryable: false, retryAfter: null };
    const msg = normalizeError(rejected).message;
    expect(msg).toBe('File not found: x');
    expect(msg).not.toContain('[object Object]');
  });
});
