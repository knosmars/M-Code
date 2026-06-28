import { create } from 'zustand';
import { typedInvoke, normalizeError } from '../utils/ipc';
import type { McpServerConfig, McpServerStatus, McpToolInfo } from '../types/ipc';

interface McpState {
  servers: McpServerConfig[];
  statuses: McpServerStatus[];
  tools: McpToolInfo[];
  loading: boolean;
  error: string | null;
  load: () => Promise<void>;
  addServer: (name: string, command: string, args: string[], env: Record<string, string>) => Promise<void>;
  removeServer: (name: string) => Promise<void>;
  setDisabled: (name: string, disabled: boolean) => Promise<void>;
  toolsForServer: (name: string) => McpToolInfo[];
}

export const useMcpStore = create<McpState>((set, get) => ({
  servers: [],
  statuses: [],
  tools: [],
  loading: false,
  error: null,

  load: async () => {
    set({ loading: true, error: null });
    try {
      const [serversRaw, statusRaw, toolsRaw] = await Promise.all([
        typedInvoke<string>('mcp_config_list'),
        typedInvoke<string>('tool_mcp_status'),
        typedInvoke<string>('tool_mcp_list_tools'),
      ]);
      set({
        servers: JSON.parse(serversRaw),
        statuses: JSON.parse(statusRaw),
        tools: JSON.parse(toolsRaw),
        loading: false,
      });
    } catch (e) {
      set({ error: normalizeError(e).message, loading: false });
    }
  },

  addServer: async (name, command, args, env) => {
    try {
      await typedInvoke<void>('mcp_config_add', { name, command, args, env });
      await get().load();
    } catch (e) {
      set({ error: normalizeError(e).message });
      throw e; // let the form keep the user's input
    }
  },

  removeServer: async (name) => {
    try {
      await typedInvoke<void>('mcp_config_remove', { name });
      await get().load();
    } catch (e) {
      set({ error: normalizeError(e).message });
    }
  },

  setDisabled: async (name, disabled) => {
    try {
      await typedInvoke<void>('mcp_config_set_disabled', { name, disabled });
      await get().load();
    } catch (e) {
      set({ error: normalizeError(e).message });
    }
  },

  toolsForServer: (name) => get().tools.filter((t) => t.name.startsWith(`mcp__${name}__`)),
}));
