import type { Message } from './message';
import type { AgentState } from '../agent/state';

/** Token usage statistics for a single LLM call or accumulated session.
 *
 * Per DEVELOPMENT_GUIDE §9, each session tracks its token consumption
 * so users can monitor API costs.
 */
export interface TokenUsage {
  /** Prompt tokens consumed */
  promptTokens: number;
  /** Completion tokens generated */
  completionTokens: number;
  /** Total tokens (prompt + completion) */
  totalTokens: number;
  /** Accumulated cost in USD */
  cost: number;
}

/** A chat session containing a full conversation history.
 *
 * Sessions are persisted locally and can be loaded/resumed.
 * Each session has a unique ID, a user-facing title, and
 * the complete list of messages exchanged.
 *
 * Per DEVELOPMENT_GUIDE §9, sessions also track the active
 * provider/model configuration, agent execution state, and
 * cumulative token usage.
 */
export interface Session {
  /** Unique session identifier (UUID) */
  id: string;
  /** User-facing title for the session (e.g. first message summary) */
  title: string;
  /** Ordered list of messages in the conversation */
  messages: Message[];
  /** Active provider ID for this session (e.g. "openai-compatible") */
  provider?: string;
  /** Active model name for this session (e.g. "gpt-4o") */
  model?: string;
  /** Current agent execution state (idle/thinking/tool_call/error/done) */
  status: AgentState;
  /** Cumulative token usage for this session */
  tokens: TokenUsage;
  /** Workspace directory path for this session — set on creation, never changed */
  workspacePath?: string;
  /** Unix timestamp in milliseconds when the session was created */
  createdAt: number;
  /** Unix timestamp in milliseconds when the session was last modified */
  updatedAt: number;
}
