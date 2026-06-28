import { describe, it, expect } from 'vitest';
import { normalizeError } from './ipc';

describe('normalizeError', () => {
  it('passes through a structured AppError object', () => {
    const e = { code: 'not_found', message: 'gone', retryable: false, retryAfter: null };
    expect(normalizeError(e)).toEqual(e);
  });

  it('fills defaults for a partial object', () => {
    const e = { code: 'rate_limited', message: 'slow', retryAfter: 30 };
    expect(normalizeError(e)).toEqual({
      code: 'rate_limited', message: 'slow', retryable: false, retryAfter: 30,
    });
  });

  it('wraps a bare string as internal', () => {
    expect(normalizeError('kaboom')).toEqual({
      code: 'internal', message: 'kaboom', retryable: false, retryAfter: null,
    });
  });

  it('wraps an unknown value as internal', () => {
    expect(normalizeError(42)).toEqual({
      code: 'internal', message: '42', retryable: false, retryAfter: null,
    });
  });
});
