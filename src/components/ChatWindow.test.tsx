// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, fireEvent, waitFor, act, within } from '@testing-library/react';
import type { StreamEvent } from '../types/stream';

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({
    minimize: vi.fn(),
    toggleMaximize: vi.fn(),
    close: vi.fn(),
  }),
}));

const h = vi.hoisted(() => ({
  scriptedStream: null as null | (() => AsyncGenerator<StreamEvent, void, unknown>),
  loadedSessions: '[]' as string,
}));

vi.mock('../hooks/useChatStream', () => ({
  useChatStream: () => ({
    streamChat: () => (h.scriptedStream ?? (async function* () {}))(),
    cancel: vi.fn(),
    isStreaming: false,
  }),
}));

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(async (cmd: string) => {
    if (cmd === 'db_load_sessions') return h.loadedSessions;
    if (cmd === 'tool_checkpoint_begin') return 'checkpoint-test-id';
    if (cmd === 'tool_checkpoint_end') return '{}';
    if (cmd === 'tool_index_codebase') return JSON.stringify({ file_count: 0, languages: {}, packages: [], entrypoints: [] });
    if (cmd === 'tool_mcp_list_tools') return '[]';
    if (cmd === 'tool_set_workspace') return '.';
    return '{}';
  }),
  Channel: class { onmessage: ((e: unknown) => void) | null = null; },
}));

import { ChatWindow } from './ChatWindow';
import { useSessionStore } from '../stores/sessionStore';
import { useProviderStore } from '../stores/providerStore';
import { useSettingsStore } from '../stores/settingsStore';
import { useViewStore } from '../stores/viewStore';
import type { Session } from '../types/session';

const delta = (content: string): StreamEvent => ({ type: 'content_delta', content });
const toolCall = (id: string, name: string, args: string): StreamEvent => ({ type: 'tool_call', id, name, arguments: args });
const done = (): StreamEvent => ({ type: 'done' } as StreamEvent);
const sError = (code: string, message: string): StreamEvent => ({ type: 'error', code, message });
function script(...events: StreamEvent[]) {
  return async function* () { for (const e of events) yield e; };
}

const SEED_SESSION: Session = {
  id: 's1', title: 'T', messages: [], model: 'm',
  status: { type: 'idle' },
  tokens: { promptTokens: 0, completionTokens: 0, totalTokens: 0, cost: 0 },
  createdAt: Date.now(), updatedAt: Date.now(),
};

function seedStores() {
  // Keep h.loadedSessions in sync so loadSessions() won't wipe the seeded session.
  h.loadedSessions = JSON.stringify([SEED_SESSION]);
  useProviderStore.setState({
    providers: [{ id: 'p', name: 'P', baseUrl: 'http://x', models: ['m'], requiresApiKey: false }],
    activeProviderId: 'p',
    selectedModel: 'm',
    initialized: true,
    initError: null,
    configuredProviders: new Set<string>(),
    fetchingModels: new Set<string>(),
    fetchErrors: {},
  });
  useSessionStore.setState({
    sessions: [{ ...SEED_SESSION, messages: [] }],
    currentSessionId: 's1',
  });
}

const mkSession = (id: string, title: string): Session => ({
  id, title, messages: [], model: 'm',
  status: { type: 'idle' },
  tokens: { promptTokens: 0, completionTokens: 0, totalTokens: 0, cost: 0 },
  createdAt: Date.now(), updatedAt: Date.now(),
});

function seedMany(...s: Session[]) {
  h.loadedSessions = JSON.stringify(s);
  useSessionStore.setState({ sessions: s, currentSessionId: s[0]?.id ?? null });
}

async function openFromSidebar(title: string) {
  await act(async () => {
    fireEvent.click(screen.getByTitle(title));
  });
}

function closeButtons() {
  const tabBar = screen.getByRole('tablist', { name: 'Tabs' });
  return within(tabBar).getAllByRole('button', { name: 'Close session tab' });
}

async function typeAndSend(container: HTMLElement, text: string) {
  const ta = container.querySelector('textarea');
  if (!ta) throw new Error('composer textarea not found');
  await act(async () => {
    fireEvent.change(ta, { target: { value: text } });
  });
  await act(async () => {
    fireEvent.click(screen.getByLabelText('Send message'));
  });
}

describe('ChatWindow integration', () => {
  beforeEach(() => {
    h.scriptedStream = null;
    seedStores();
    vi.clearAllMocks();
    useViewStore.setState({ view: 'chat', previous: 'chat' });
  });

  it('sends a message, renders the streamed reply, and persists it', async () => {
    h.scriptedStream = script(delta('Hello'), done());
    const { container } = render(<ChatWindow />);

    await typeAndSend(container, 'hi');

    await waitFor(() => expect(screen.getByText('Hello')).toBeTruthy());

    await waitFor(() => {
      const msgs = useSessionStore.getState().sessions.find((s) => s.id === 's1')!.messages;
      const assistant = msgs.find(
        (m) => m.role === 'assistant' && typeof m.content === 'string' && m.content.includes('Hello'),
      );
      expect(assistant).toBeTruthy();
    });
  });

  it('opens the permission dialog for a side-effecting tool call', async () => {
    h.scriptedStream = script(
      toolCall('t1', 'write_file', JSON.stringify({ path: 'a.txt', content: 'x' })),
      done(),
    );
    const { container } = render(<ChatWindow />);

    await typeAndSend(container, 'write a file');

    // PermissionDialog (role="dialog") shows the pending tool's name.
    await waitFor(() => {
      const dialog = screen.getByRole('dialog');
      expect(dialog.textContent).toContain('write_file');
    });
  });

  it('surfaces a stream error in the error banner', async () => {
    h.scriptedStream = script(sError('provider', 'boom'));
    const { container } = render(<ChatWindow />);

    await typeAndSend(container, 'hi');

    await waitFor(() => expect(screen.getByText(/boom/)).toBeTruthy());
  });

  it('clicking a session tab keeps the chat view instead of a blank file editor', async () => {
    const { container } = render(<ChatWindow />);

    // Open the session from the sidebar → it becomes a tab in the tab bar.
    await act(async () => {
      fireEvent.click(screen.getByTitle('T'));
    });

    // Click that session tab in the main tab bar (the one with a close button).
    const tabBar = screen.getByRole('tablist', { name: 'Tabs' });
    const sessionTab = within(tabBar)
      .getAllByRole('tab')
      .find((el) => el.querySelector('[aria-label="Close session tab"]'));
    if (!sessionTab) throw new Error('session tab not found');
    await act(async () => {
      fireEvent.click(sessionTab);
    });

    // The chat composer must still be rendered (chat view), and the broken
    // "File not found" code-editor fallback must NOT appear.
    expect(container.querySelector('textarea')).toBeTruthy();
    expect(screen.queryByText('File not found')).toBeNull();
  });

  it('does not render a separate Chat tab — session tabs are the conversations', async () => {
    render(<ChatWindow />);

    // Open a session → the tab bar appears with just that session tab.
    await act(async () => {
      fireEvent.click(screen.getByTitle('T'));
    });

    const tabBar = screen.getByRole('tablist', { name: 'Tabs' });
    expect(within(tabBar).queryByText('Chat')).toBeNull();
  });

  it('keeps a streaming reply in its own session when you switch tabs mid-stream', async () => {
    // Make rAF synchronous so the ephemeral streaming bubble renders mid-stream.
    const origRaf = globalThis.requestAnimationFrame;
    globalThis.requestAnimationFrame = ((cb: FrameRequestCallback) => { cb(0); return 0; }) as typeof globalThis.requestAnimationFrame;
    try {
      seedMany(mkSession('s1', 'A'), mkSession('s2', 'B'));
      // A stream that emits a partial reply then stays open (never completes).
      let release: () => void = () => {};
      const gate = new Promise<void>((r) => { release = r; });
      h.scriptedStream = async function* () { yield delta('PARTIAL_REPLY'); await gate; };

      const { container } = render(<ChatWindow />);
      await openFromSidebar('A');
      await openFromSidebar('B');

      // Go to A and send → stream starts for s1.
      const tabBar = screen.getByRole('tablist', { name: 'Tabs' });
      await act(async () => { fireEvent.click(within(tabBar).getAllByRole('tab')[0]); });
      expect(useSessionStore.getState().currentSessionId).toBe('s1');
      await typeAndSend(container, 'hi from A');
      await waitFor(() => expect(screen.getByText('PARTIAL_REPLY')).toBeTruthy());

      // Switch to B mid-stream → the partial reply must NOT leak into B.
      await act(async () => { fireEvent.click(within(tabBar).getAllByRole('tab')[1]); });
      expect(useSessionStore.getState().currentSessionId).toBe('s2');
      expect(screen.queryByText('PARTIAL_REPLY')).toBeNull();

      release();
    } finally {
      globalThis.requestAnimationFrame = origRaf;
    }
  });

  it('closing the active session tab switches to the left-neighbor tab', async () => {
    seedMany(mkSession('s1', 'A'), mkSession('s2', 'B'), mkSession('s3', 'C'));
    render(<ChatWindow />);
    await openFromSidebar('A');
    await openFromSidebar('B');
    await openFromSidebar('C');
    expect(useSessionStore.getState().currentSessionId).toBe('s3');

    // Close C (active, rightmost) → jump to its left neighbor B.
    await act(async () => {
      fireEvent.click(closeButtons()[2]);
    });
    expect(useSessionStore.getState().currentSessionId).toBe('s2');
    // The closed session is preserved (not deleted) — still in the sidebar/store.
    expect(useSessionStore.getState().sessions.some((s) => s.id === 's3')).toBe(true);
  });

  it('closing the leftmost active tab falls to the new first tab', async () => {
    seedMany(mkSession('s1', 'A'), mkSession('s2', 'B'), mkSession('s3', 'C'));
    render(<ChatWindow />);
    await openFromSidebar('A');
    await openFromSidebar('B');
    await openFromSidebar('C');

    // Make the first tab (s1) active, then close it → no left neighbor → new first (s2).
    const tabBar = screen.getByRole('tablist', { name: 'Tabs' });
    await act(async () => {
      fireEvent.click(within(tabBar).getAllByRole('tab')[0]);
    });
    expect(useSessionStore.getState().currentSessionId).toBe('s1');
    await act(async () => {
      fireEvent.click(closeButtons()[0]);
    });
    expect(useSessionStore.getState().currentSessionId).toBe('s2');
  });

  it('closing the last remaining session tab returns to the welcome home', async () => {
    render(<ChatWindow />);
    await openFromSidebar('T'); // seedStores already provides session s1 titled "T"
    expect(useSessionStore.getState().currentSessionId).toBe('s1');

    await act(async () => {
      fireEvent.click(closeButtons()[0]);
    });
    expect(useSessionStore.getState().currentSessionId).toBeNull();
  });

  it('clicking a welcome capability card fills the composer', async () => {
    // No active session → the welcome screen renders.
    useSessionStore.setState({ sessions: [], currentSessionId: null });
    const { container } = render(<ChatWindow />);

    await act(async () => {
      fireEvent.click(screen.getByText('用语义搜索找出处理支付的代码'));
    });

    const ta = container.querySelector('textarea') as HTMLTextAreaElement;
    expect(ta.value).toBe('用语义搜索找出处理支付的代码');
  });

  it('localizes the error-banner label via useT when language is en', async () => {
    useSettingsStore.setState({ language: 'en' });
    h.scriptedStream = script(sError('provider', 'boom'));
    const { container } = render(<ChatWindow />);

    await typeAndSend(container, 'hi');

    // 'provider' code → chat.error.provider → en 'Provider error' (not zh '服务商错误').
    await waitFor(() => expect(screen.getByText(/Provider error/)).toBeTruthy());
    expect(screen.queryByText(/服务商错误/)).toBeNull();
    useSettingsStore.setState({ language: 'zh' });
  });
});
