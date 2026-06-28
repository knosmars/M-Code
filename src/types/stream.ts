import type { TokenUsage } from './session';

/** Events streamed from Rust backend through Tauri Channel to the frontend. */
export type StreamEvent =
  | { type: 'content_delta'; content: string }
  | { type: 'reasoning_delta'; content: string }
  | { type: 'tool_call'; id: string; name: string; arguments: string }
  | { type: 'tool_result'; id: string; content: string }
  | { type: 'error'; code: string; message: string; retryable?: boolean }
  | { type: 'done'; usage?: TokenUsage };
