// @vitest-environment jsdom
import { describe, it, expect } from 'vitest';
import { act, renderHook } from '@testing-library/react';
import { usePermissionGate } from './usePermissionGate';
import type { ToolCall } from '../types/message';
import type { PermissionDecision } from '../agent/tools';

const editCall: ToolCall = { id: 'c1', name: 'write_file', arguments: '{}' };

describe('usePermissionGate', () => {
  it('auto mode auto-approves without opening a dialog', async () => {
    const { result } = renderHook(() => usePermissionGate());
    act(() => result.current.setPermissionMode('auto'));
    let decision: PermissionDecision | undefined;
    await act(async () => {
      decision = await result.current.requestPermission(editCall);
    });
    expect(decision).toBe('allow');
    expect(result.current.pendingPermission).toBeNull();
  });

  it('accept-edits mode auto-approves edit tools', async () => {
    const { result } = renderHook(() => usePermissionGate());
    act(() => result.current.setPermissionMode('accept-edits'));
    let decision: PermissionDecision | undefined;
    await act(async () => {
      decision = await result.current.requestPermission(editCall);
    });
    expect(decision).toBe('allow');
  });

  it('ask mode opens a dialog and resolves on the user decision', async () => {
    const { result } = renderHook(() => usePermissionGate());
    let resolved: PermissionDecision | undefined;
    act(() => {
      void result.current.requestPermission(editCall).then((d) => { resolved = d; });
    });
    expect(result.current.pendingPermission).toEqual(editCall);
    await act(async () => {
      result.current.resolvePermission('allow');
    });
    expect(resolved).toBe('allow');
    expect(result.current.pendingPermission).toBeNull();
  });

  it('denyPending denies an open request', async () => {
    const { result } = renderHook(() => usePermissionGate());
    let resolved: PermissionDecision | undefined;
    act(() => {
      void result.current.requestPermission(editCall).then((d) => { resolved = d; });
    });
    await act(async () => {
      result.current.denyPending();
    });
    expect(resolved).toBe('deny');
    expect(result.current.pendingPermission).toBeNull();
  });
});
