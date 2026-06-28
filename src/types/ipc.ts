import type { ToolDefinition } from '../agent/tools';
import type { ContentPart } from './message';

/** A message as sent to the LLM API — only the fields the Rust ChatMessage struct expects.
 *
 *  Frontend Message has extra fields (id, timestamp, toolCalls) that cause
 *  serde deserialization errors. The Rust ChatMessage only expects:
 *  role (string), content (string | ContentPart[]), toolCallId (optional string).
 */
export interface ApiMessage {
  role: string;
  content: string | ContentPart[];
  toolCallId?: string;
  name?: string;
}

/** A tool in OpenAI function-calling format.
 *
 * All provider adapters (OpenAI, Anthropic, Gemini) expect this wrapped
 * shape — OpenAI sends it verbatim; Anthropic/Gemini read `.function`.
 * Sending a bare {@link ToolDefinition} causes the API to reject the request
 * with "tools[0].type: unknown variant".
 */
export interface OpenAITool {
  type: 'function';
  function: ToolDefinition;
}

/** Request sent from the frontend to the Rust `stream_chat` Tauri command.
 *
 * Contains all information needed to start an LLM streaming session:
 * the conversation history, provider/model selection, optional
 * system prompt, and optional tool definitions for function calling.
 */
export interface ChatRequest {
  /** The session ID this chat belongs to */
  sessionId: string;
  /** Full conversation history to send to the LLM (only role, content, toolCallId) */
  messages: ApiMessage[];
  /** Provider identifier (e.g. "openai-compatible") */
  providerId: string;
  /** Model name (e.g. "gpt-4o") */
  model: string;
  /** Optional system prompt to prepend to the conversation */
  systemPrompt?: string;
  /** Optional tool definitions for function calling (DEVELOPMENT_GUIDE §7) */
  tools?: OpenAITool[];
  /** Optional provider base URL override (local Ollama / custom gateway). */
  baseUrl?: string;
}

/** Request to execute a tool on the Rust backend.
 *
 * Sent when the LLM requests a tool call and the agent loop
 * needs to execute it server-side.
 */
export interface ToolRequest {
  /** The name of the tool to execute */
  name: string;
  /** JSON-encoded arguments for the tool */
  arguments: string;
}

/** Response from a tool execution on the Rust backend.
 *
 * Contains the execution result or error information.
 */
export interface ToolResponse {
  /** Unique identifier matching the tool call */
  id: string;
  /** The name of the tool that was executed */
  name: string;
  /** The output content from the tool execution */
  content: string;
  /** Error message if the tool execution failed */
  error?: string;
}

/** Remote info for the Git status popup — camelCase mirror of Rust `GitRemoteInfo`. */
export interface GitRemoteInfo {
  remoteUrl: string;
  owner: string;
  repo: string;
  branch: string;
}

/** GitHub CLI auth status — mirror of Rust `GhAuthInfo`. */
export interface GhAuthStatus {
  loggedIn: boolean;
  username: string;
}

/** OAuth login / status result — mirror of Rust `OAuthStatus`. */
export interface OAuthStatus {
  loggedIn: boolean;
  username: string | null;
}

/** Structured error shape that Rust `AppError` serializes to (camelCase wire). */
export interface AppErrorShape {
  code: string;
  message: string;
  retryable: boolean;
  retryAfter: number | null;
}

/** A configured MCP server (mirror of Rust config entry). */
export interface McpServerConfig {
  name: string;
  command: string;
  args: string[];
  env: Record<string, string>;
  disabled: boolean;
}

/** Live connection state of an MCP server. */
export interface McpServerStatus {
  name: string;
  connected: boolean;
  toolCount: number;
  disabled: boolean;
}

/** A tool discovered from an MCP server (namespaced `mcp__<server>__<tool>`). */
export interface McpToolInfo {
  name: string;
  description: string;
  parameters: unknown;
}

/** Response from `tool_semantic_status` — snake_case mirrors Rust serde JSON keys. */
export interface SemanticStatus {
  indexed: boolean;
  file_count: number;
  chunk_count: number;
  embed_model: string | null;
  embed_dim: number | null;
}

/** Embedding configuration — snake_case mirrors Rust serde keys for `tool_semantic_config_get/set`. */
export interface SemanticConfig {
  embed_base: string;
  embed_model: string;
}
