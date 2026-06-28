import { describe, it, expect } from 'vitest';
import { detectAtToken } from './mention';

describe('detectAtToken', () => {
  it('detects @ at the start of input', () => {
    const v = '@src';
    expect(detectAtToken(v, v.length)).toEqual({ query: 'src', start: 0 });
  });

  it('detects @ after whitespace mid-text', () => {
    const v = 'see @main';
    expect(detectAtToken(v, v.length)).toEqual({ query: 'main', start: 4 });
  });

  it('returns empty query right after a lone @', () => {
    const v = 'open @';
    expect(detectAtToken(v, v.length)).toEqual({ query: '', start: 5 });
  });

  it('does NOT trigger on email-like text (@ not preceded by space)', () => {
    const v = 'mail user@example';
    expect(detectAtToken(v, v.length)).toBeNull();
  });

  it('does NOT trigger when whitespace follows the @ token', () => {
    const v = '@src done';
    expect(detectAtToken(v, v.length)).toBeNull();
  });

  it('uses the caret, not end of string', () => {
    const v = '@src and more';
    expect(detectAtToken(v, 4)).toEqual({ query: 'src', start: 0 });
  });

  it('returns null with no @ token', () => {
    const v = 'plain text';
    expect(detectAtToken(v, v.length)).toBeNull();
  });
});
