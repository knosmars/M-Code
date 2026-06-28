import { useState, useRef, useEffect, useCallback } from 'react';
import type { ToolCall } from '../types/message';
import type { PermissionDecision } from '../agent/tools';

export type PermissionMode = 'ask' | 'accept-edits' | 'plan' | 'auto';

const EDIT_TOOL_NAMES = ['write_file', 'edit_file', 'create_file'];

/**
 * The permission gate that pauses the agent loop before side-effecting tools.
 *
 * `requestPermission` is the callback the loop awaits: it auto-approves in
 * `auto` mode (and file edits in `accept-edits` mode), otherwise it surfaces a
 * dialog and resolves once the user decides. Extracted from ChatWindow.
 */
export function usePermissionGate() {
  const [pendingPermission, setPendingPermission] = useState<ToolCall | null>(null);
  const [permissionMode, setPermissionMode] = useState<PermissionMode>('ask');
  const permissionResolveRef = useRef<((decision: PermissionDecision) => void) | null>(null);

  // Mirror the mode into a ref so the streaming callback always reads the
  // current value rather than the one captured when the turn began.
  const permissionModeRef = useRef(permissionMode);
  useEffect(() => { permissionModeRef.current = permissionMode; }, [permissionMode]);

  const editTools = useRef(new Set(EDIT_TOOL_NAMES)).current;

  /** Callback the agent loop awaits before running a side-effecting tool. */
  const requestPermission = useCallback((toolCall: ToolCall): Promise<PermissionDecision> => {
    if (permissionModeRef.current === 'auto') {
      return Promise.resolve('allow' as PermissionDecision);
    }
    if (permissionModeRef.current === 'accept-edits' && editTools.has(toolCall.name)) {
      return Promise.resolve('allow' as PermissionDecision);
    }
    return new Promise<PermissionDecision>((resolve) => {
      permissionResolveRef.current = resolve;
      setPendingPermission(toolCall);
    });
  }, [editTools]);

  /** Resolve the pending dialog with the user's decision. */
  const resolvePermission = useCallback((decision: PermissionDecision) => {
    permissionResolveRef.current?.(decision);
    permissionResolveRef.current = null;
    setPendingPermission(null);
  }, []);

  /** Deny any pending request (used when the turn is cancelled). */
  const denyPending = useCallback(() => {
    if (permissionResolveRef.current) {
      permissionResolveRef.current('deny');
      permissionResolveRef.current = null;
      setPendingPermission(null);
    }
  }, []);

  return {
    pendingPermission,
    permissionMode,
    setPermissionMode,
    requestPermission,
    resolvePermission,
    denyPending,
  };
}
