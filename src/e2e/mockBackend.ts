import { mockIPC } from '@tauri-apps/api/mocks';
import type { StreamEvent } from '../types/stream';

declare global {
  interface Window { __E2E_SCENARIO__?: string }
}

interface Channel { onmessage: (e: StreamEvent) => void }

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

const E2E_SESSIONS_KEY = 'e2e:sessions';

function e2eReadSessions(): Record<string, unknown> {
  try {
    return JSON.parse(localStorage.getItem(E2E_SESSIONS_KEY) ?? '{}') as Record<string, unknown>;
  } catch {
    return {};
  }
}

function e2eWriteSessions(map: Record<string, unknown>): void {
  localStorage.setItem(E2E_SESSIONS_KEY, JSON.stringify(map));
}

async function driveTextStream(ch: Channel): Promise<void> {
  for (const part of ['Hello', ', ', 'world', '!']) {
    ch.onmessage({ type: 'content_delta', content: part });
    await sleep(20);
  }
  ch.onmessage({ type: 'done' });
}

let streamRound = 0;

async function driveToolStream(ch: Channel): Promise<void> {
  streamRound += 1;
  if (streamRound === 1) {
    ch.onmessage({ type: 'content_delta', content: '我来写文件。' });
    ch.onmessage({
      type: 'tool_call',
      id: 'call_1',
      name: 'write_file',
      arguments: JSON.stringify({ path: 'demo.txt', content: 'hi' }),
    });
    ch.onmessage({ type: 'done' });
  } else {
    ch.onmessage({ type: 'content_delta', content: '完成。' });
    ch.onmessage({ type: 'done' });
  }
}

/** Install the e2e IPC mock. Loaded only when VITE_E2E is set. */
export function setupMockBackend(): void {
  // Reset round counter so each page load starts fresh.
  streamRound = 0;

  mockIPC((cmd, payload) => {
    switch (cmd) {
      case 'get_api_key':
        return 'sk-e2e';                                   // every provider configured
      case 'set_api_key':
        return null;
      case 'list_models':
        return JSON.stringify({ models: ['e2e-model'] });
      case 'db_load_sessions':
        return JSON.stringify(Object.values(e2eReadSessions()));
      case 'db_save_session': {
        const { json } = payload as { json: string };
        const session = JSON.parse(json) as { id: string };
        const map = e2eReadSessions();
        map[session.id] = session;
        e2eWriteSessions(map);
        return null;
      }
      case 'db_delete_session': {
        const { id } = payload as { id: string };
        const map = e2eReadSessions();
        delete map[id];
        e2eWriteSessions(map);
        return null;
      }
      case 'tool_index_codebase':
        return JSON.stringify({ file_count: 0, languages: {}, packages: [], entrypoints: [] });
      case 'tool_set_workspace':
        return '.';
      case 'tool_mcp_list_tools':
        return '[]';
      case 'tool_checkpoint_begin':
        return 'e2e-checkpoint-id';
      case 'tool_checkpoint_end':
        return null;
      // Internal tool invocations called by the agent loop (best-effort; null
      // return is also safe since the loop wraps these in try/catch, but
      // explicit mocks prevent noisy console errors in e2e output).
      case 'tool_agents_rules_read':
        return JSON.stringify({ system_prompt: '' });
      case 'tool_memory_read':
        return '';
      case 'tool_hooks_run':
        return JSON.stringify({ output: '', blocked: false });
      case 'tool_agents_list':
        return JSON.stringify({ agents: [] });
      // write_file tool: execution path + fileSync side-effects.
      case 'tool_write_file':
        return 'wrote demo.txt';
      case 'tool_file_sync_register':
        return 'ok';
      case 'tool_file_sync_publish':
        return [];
      case 'stream_chat': {
        const ch = (payload as { onEvent: Channel }).onEvent;
        // Return the Promise so mockIPC resolves only after the stream is fully
        // driven. If we fire-and-forget (void), invokeDone is set immediately
        // and the useChatStream generator exits before all deltas are delivered.
        const scenario = window.__E2E_SCENARIO__;
        if (scenario === 'tool') {
          return driveToolStream(ch);
        }
        return driveTextStream(ch);
      }
      // ── Settings panel IPC (visual e2e) ──
      case 'mcp_config_list':
        return '[]';
      case 'tool_mcp_status':
        return '[]';
      case 'mcp_config_add':
      case 'mcp_config_remove':
      case 'mcp_config_set_disabled':
        return null;
      case 'tool_semantic_status':
        return { indexed: false, file_count: 0, chunk_count: 0, embed_model: null, embed_dim: null };
      case 'tool_semantic_config_get':
        return { embed_base: 'http://localhost:11434', embed_model: 'nomic-embed-text' };
      case 'tool_semantic_config_set':
        return null;
      case 'tool_semantic_index':
        return 'indexed (e2e)';
      case 'delete_api_key':
        return null;
      default:
        return null;                                       // benign default; expand as flows need
    }
  });
}
