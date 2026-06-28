/**
 * Detect an `@file` mention token ending at the caret position, if any.
 * Returns the partial query after `@` and the index of the `@` in `value`.
 */
export function detectAtToken(value: string, caret: number): { query: string; start: number } | null {
  const upto = value.slice(0, caret);
  const m = /(?:^|\s)@([^\s@]*)$/.exec(upto);
  if (!m) return null;
  const query = m[1];
  return { query, start: caret - query.length - 1 };
}
