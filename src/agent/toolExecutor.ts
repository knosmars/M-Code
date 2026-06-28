import type { ToolCall, ToolResult } from '../types/message';
import type { ToolDefinition, ToolExecutor } from './tools';
import { TOOL_REGISTRY, REGISTRY_DEFS, type ToolSpec } from './toolRegistry';
import { typedInvoke, normalizeError } from '../utils/ipc';

const REGISTRY = new Map<string, ToolSpec>(TOOL_REGISTRY.map((s) => [s.definition.name, s]));

/**
 * Concrete ToolExecutor that invokes Tauri backend tool commands.
 *
 * Maps frontend tool definitions → Rust `#[tauri::command]` handlers.
 * Handles parameter name mismatches between frontend tool schemas
 * (sent to LLM) and Rust function signatures.
 */
export class TauriToolExecutor implements ToolExecutor {
  /** Tools discovered from configured MCP servers (namespaced `mcp__...`). */
  private mcpTools: ToolDefinition[] = [];

  /**
   * Load tools from configured MCP servers into the executor. Best-effort:
   * on any failure the built-in tools remain available. Call once when a
   * workspace/session initializes.
   */
  async loadMcpTools(): Promise<void> {
    try {
      const raw = await typedInvoke<string>('tool_mcp_list_tools');
      const parsed = JSON.parse(raw) as ToolDefinition[];
      this.mcpTools = Array.isArray(parsed) ? parsed : [];
    } catch {
      this.mcpTools = [];
    }
  }

  getDefinitions(): ToolDefinition[] {
    return this.mcpTools.length > 0 ? [...REGISTRY_DEFS, ...this.mcpTools] : REGISTRY_DEFS;
  }

  hasSideEffect(toolName: string): boolean {
    if (toolName.startsWith('mcp__')) return true;
    return REGISTRY.get(toolName)?.sideEffect ?? false;
  }

  async execute(toolCall: ToolCall): Promise<ToolResult> {
    let args: Record<string, unknown>;

    try {
      args = JSON.parse(toolCall.arguments);
    } catch {
      return {
        toolCallId: toolCall.id,
        content: `Tool argument parse error: invalid JSON — ${toolCall.arguments.slice(0, 200)}`,
      };
    }

    try {
      const content = await this.dispatch(toolCall.name, args);
      return { toolCallId: toolCall.id, content };
    } catch (err: unknown) {
      const message = normalizeError(err).message;
      return { toolCallId: toolCall.id, content: `Tool execution error: ${message}` };
    }
  }

  /** Route tool call to the correct Tauri command with parameter mapping. */
  private async dispatch(name: string, args: Record<string, unknown>): Promise<string> {
    // MCP tools are namespaced `mcp__<server>__<tool>` and proxied to the
    // backend MCP client, which forwards the JSON arguments unchanged.
    if (name.startsWith('mcp__')) {
      return typedInvoke<string>('tool_mcp_call', { name, arguments: JSON.stringify(args) });
    }

    const spec = REGISTRY.get(name);
    if (!spec) {
      throw new Error(`Unknown tool: ${name}`);
    }
    return spec.invoke(args);
  }
}
