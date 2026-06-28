import type { ToolCall, ToolResult } from '../types/message';

/** OpenAI-compatible function definition for a tool.
 *
 * Sent to the LLM as part of the request so it knows which
 * functions it can call. Matches the OpenAI `tools[].function` schema.
 */
export interface ToolDefinition {
  name: string;
  description: string;
  parameters: Record<string, unknown>;
}

/** Decision returned by the permission gate for a tool call. */
export type PermissionDecision = 'allow' | 'deny' | 'always_allow';

/** Interface for tool execution.
 *
 * Injected into AgentLoop so it can be swapped for testing
 * or extended with additional tools without changing the loop logic.
 */
export interface ToolExecutor {
  /** Execute a tool call and return the result */
  execute(toolCall: ToolCall): Promise<ToolResult>;

  /** Return the tool definitions to send to the LLM */
  getDefinitions(): ToolDefinition[];

  /** Does this tool have side effects (writes, shell commands)? */
  hasSideEffect(toolName: string): boolean;
}
