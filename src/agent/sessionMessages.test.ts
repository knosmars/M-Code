import { describe, it, expect } from 'vitest';
import { buildUserContent, buildPartialMessages, buildDoneMessages } from './sessionMessages';
import type { ToolCall, ToolResult } from '../types/message';

const opts = { uuid: () => 'fixed-id', now: () => 123 };

const call: ToolCall = { id: 'tc1', name: 'write_file', arguments: '{}' };
const result: ToolResult = { toolCallId: 'tc1', content: 'ok' };

describe('buildUserContent', () => {
  it('returns a plain string when there are no images', () => {
    expect(buildUserContent('hello', [])).toBe('hello');
  });

  it('returns a parts array with text first, then images', () => {
    const parts = buildUserContent('hi', [{ dataUrl: 'data:img1' }]);
    expect(parts).toEqual([
      { type: 'text', text: 'hi' },
      { type: 'image_url', image_url: { url: 'data:img1' } },
    ]);
  });

  it('omits the text part when text is empty', () => {
    const parts = buildUserContent('', [{ dataUrl: 'data:img1' }]);
    expect(parts).toEqual([{ type: 'image_url', image_url: { url: 'data:img1' } }]);
  });
});

describe('buildPartialMessages', () => {
  it('returns [] when there is nothing to save', () => {
    expect(buildPartialMessages('a1', '', [], [], opts)).toEqual([]);
  });

  it('marks interrupted content and carries tool messages with id + name', () => {
    const msgs = buildPartialMessages('a1', 'partial', [call], [result], opts);
    expect(msgs[0]).toMatchObject({
      id: 'a1',
      role: 'assistant',
      content: 'partial\n\n[interrupted]',
      toolCalls: [call],
    });
    expect(msgs[1]).toMatchObject({
      id: 'fixed-id',
      role: 'tool',
      content: 'ok',
      toolCallId: 'tc1',
      name: 'write_file',
    });
  });

  it('saves tool results even with empty assistant content', () => {
    const msgs = buildPartialMessages('a1', '', [call], [result], opts);
    expect(msgs[0].content).toBe('');
    expect(msgs).toHaveLength(2);
  });
});

describe('buildDoneMessages', () => {
  it('persists the assistant message without an interrupted marker', () => {
    const msgs = buildDoneMessages('a1', 'final', [call], [result], opts);
    expect(msgs[0]).toMatchObject({ id: 'a1', role: 'assistant', content: 'final', toolCalls: [call] });
    expect(msgs[1]).toMatchObject({ role: 'tool', toolCallId: 'tc1', name: 'write_file' });
  });

  it('omits name when no matching tool call is found', () => {
    const orphan: ToolResult = { toolCallId: 'unknown', content: 'x' };
    const msgs = buildDoneMessages('a1', '', [], [orphan], opts);
    expect(msgs[1]).not.toHaveProperty('name');
    expect(msgs[1]).toMatchObject({ role: 'tool', toolCallId: 'unknown' });
  });

  it('drops toolCalls field when there are none', () => {
    const msgs = buildDoneMessages('a1', 'hi', [], [], opts);
    expect(msgs[0].toolCalls).toBeUndefined();
    expect(msgs).toHaveLength(1);
  });
});
