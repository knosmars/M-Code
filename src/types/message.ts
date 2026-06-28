/** A content part for multimodal messages (text + images). */
export interface ContentPart {
  type: 'text' | 'image_url';
  text?: string;
  image_url?: { url: string; detail?: string };
}

/** Extract plain text from a message's content, ignoring image parts. */
export function getTextContent(content: string | ContentPart[]): string {
  if (typeof content === 'string') return content;
  return content.filter(p => p.type === 'text').map(p => p.text ?? '').join('');
}

/** Extract image URLs from a message's content parts. */
export function getImageParts(content: string | ContentPart[]): { url: string; detail?: string }[] {
  if (typeof content === 'string') return [];
  return content
    .filter(p => p.type === 'image_url' && p.image_url?.url)
    .map(p => ({ url: p.image_url!.url, detail: p.image_url?.detail }));
}

/** A single chat message in a conversation.
 *
 * Represents one turn in the conversation. Messages with
 * `role: 'assistant'` may include optional `toolCalls` when
 * the LLM requests function execution. Messages with
 * `role: 'tool'` include `toolCallId` to link back to the
 * tool call they fulfill.
 */
export interface Message {
  /** Unique identifier for this message */
  id: string;
  /** The role of the message author */
  role: 'user' | 'assistant' | 'system' | 'tool';
  /** The text content of the message (or multimodal content parts) */
  content: string | ContentPart[];
  /** Tool calls requested by the assistant (only present on assistant messages) */
  toolCalls?: ToolCall[];
  /** ID of the tool call this message fulfills (only present on tool messages) */
  toolCallId?: string;
  /** Tool function name (only present on tool messages, required by Gemini API) */
  name?: string;
  /** Reasoning/thinking content from the model (e.g. DeepSeek reasoning mode) */
  reasoningContent?: string;
  /** Terminal snapshot of a parallel dispatch issued by this message (persisted). */
  parallelSnapshot?: import('../agent/parallelEvents').ParallelAgentState[];
  /** Checkpoint id covering this turn's file edits (assistant messages) —
   *  enables the "revert files" button. Absent when the turn made no checkpoint. */
  checkpointId?: string;
  /** Unix timestamp in milliseconds when the message was created */
  timestamp: number;
}

/** A tool call requested by the LLM during function calling.
 *
 * Contains the function name and its JSON-encoded arguments.
 * The frontend serializes these and the backend executes them.
 */
export interface ToolCall {
  /** Unique identifier for this tool call (matches ToolResult.toolCallId) */
  id: string;
  /** The name of the tool/function to call */
  name: string;
  /** JSON-encoded string of the tool arguments */
  arguments: string;
}

/** The result of executing a tool call.
 *
 * Returned by the backend after tool execution and streamed
 * back to the LLM as context for its next response.
 */
export interface ToolResult {
  /** Matches the ToolCall.id that triggered this execution */
  toolCallId: string;
  /** The output content from the tool execution */
  content: string;
}

/** A planning step for autonomous task decomposition. */
export interface PlanStep {
  /** Unique step identifier */
  id: number;
  /** What to accomplish in this step */
  description: string;
  /** Current status of this step */
  status: 'pending' | 'in_progress' | 'completed' | 'failed';
  /** Result output or error message after execution */
  result?: string;
}
