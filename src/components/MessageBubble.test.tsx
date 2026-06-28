// @vitest-environment jsdom
import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { render, screen } from '@testing-library/react';
import { MessageBubble } from './MessageBubble';
import type { Message } from '../types/message';
import { useSettingsStore } from '../stores/settingsStore';

function msg(over: Partial<Message>): Message {
  return { id: 'm1', role: 'assistant', content: '', timestamp: 1_700_000_000_000, ...over };
}

beforeEach(() => {
  useSettingsStore.setState({ hideToolStderr: false, showToolCalls: true });
});

describe('MessageBubble', () => {
  it('renders a user message bubble with its text', () => {
    render(<MessageBubble message={msg({ role: 'user', content: 'hello user' })} />);
    expect(screen.getByText('hello user')).toBeTruthy();
  });

  it('renders a system message body', () => {
    render(<MessageBubble message={msg({ role: 'system', content: 'system note' })} />);
    expect(screen.getByText('system note')).toBeTruthy();
  });

  it('renders an assistant message with markdown text', () => {
    render(<MessageBubble message={msg({ role: 'assistant', content: 'assistant reply' })} />);
    expect(screen.getByText('assistant reply')).toBeTruthy();
  });

  it('renders a collapsible tool-call line for assistant tool calls', () => {
    const m = msg({
      role: 'assistant',
      content: 'doing work',
      toolCalls: [{ id: 't1', name: 'read_file', arguments: '{"path":"a.txt"}' }],
    });
    render(<MessageBubble message={m} />);
    // verb('read_file') === 'read' → label "read read_file"
    expect(screen.getByText('read read_file')).toBeTruthy();
  });

  it('returns null for an empty non-streaming message', () => {
    const { container } = render(<MessageBubble message={msg({ role: 'assistant', content: '' })} />);
    expect(container.firstChild).toBeNull();
  });

  it('shows rollback button in English with "Roll back this turn\'s file changes" title', () => {
    useSettingsStore.setState({ language: 'en' });
    const m = msg({
      role: 'assistant',
      content: 'made changes',
      checkpointId: 'cp-1',
    });
    render(<MessageBubble message={m} onRevertCheckpoint={() => {}} />);
    expect(screen.getByTitle("Roll back this turn's file changes")).toBeTruthy();
  });

  it('shows rollback button in Chinese with "回滚此轮文件改动" title', () => {
    useSettingsStore.setState({ language: 'zh' });
    const m = msg({
      role: 'assistant',
      content: 'made changes',
      checkpointId: 'cp-1',
    });
    render(<MessageBubble message={m} onRevertCheckpoint={() => {}} />);
    expect(screen.getByTitle('回滚此轮文件改动')).toBeTruthy();
  });

  afterEach(() => {
    useSettingsStore.setState({ language: 'zh' });
  });
});
