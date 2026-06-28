/** Agent execution state machine per DEVELOPMENT_GUIDE §6.2.
 *
 * Tracks the current phase of the agent loop so the UI can
 * render appropriate status indicators, loading states, and
 * error banners.
 *
 * States:
 * - `idle`:     Agent is not running (default).
 * - `thinking`: LLM is generating a response.
 * - `tool_call`: LLM requested a tool execution.
 * - `error`:    A non-recoverable error occurred.
 * - `done`:     Agent completed the turn successfully.
 */
export type AgentState =
  | { type: 'idle' }
  | { type: 'thinking'; message: string }
  | { type: 'tool_call'; tool: string; args: unknown }
  | { type: 'error'; message: string }
  | { type: 'done' };
