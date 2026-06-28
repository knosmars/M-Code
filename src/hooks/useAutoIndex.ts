import { useCallback, useEffect, useRef } from 'react';
import { typedInvoke } from '../utils/ipc';

/** 自动后台索引周期：5 分钟。 */
export const AUTO_INDEX_INTERVAL_MS = 5 * 60 * 1000;

/**
 * 启用时在工作区变化与每 {@link AUTO_INDEX_INTERVAL_MS} 后台跑增量语义索引
 * （fire-and-forget，不阻 UI）。`tool_semantic_index` 已 mtime 增量，无变更时廉价。
 * 失败静默（embedding 端点不在时不骚扰，状态由 SemanticIndexSection 反映）；
 * in-flight 守卫防一次未完时重叠触发。
 */
export function useAutoIndex(workspacePath: string, enabled: boolean): void {
  const runningRef = useRef(false);

  const runIndex = useCallback(async () => {
    if (runningRef.current) return;
    runningRef.current = true;
    try {
      // path 固定 '.'（进程 cwd，由 tool_set_workspace 设为当前工作区）——
      // 勿改成 workspacePath：相对 cwd 契约是 load-bearing。
      await typedInvoke<string>('tool_semantic_index', { path: '.' });
    } catch {
      // 静默：端点不在/索引失败不骚扰用户
    } finally {
      runningRef.current = false;
    }
  }, []);

  useEffect(() => {
    if (!enabled) return;
    // workspacePath 入依赖以在切换工作区时重跑一次（mount 时 '.'→canonical 解析
    // 会触发两次，但 in-flight 守卫使第二次为 no-op，且增量无变更廉价）。
    void runIndex();
    const id = setInterval(() => void runIndex(), AUTO_INDEX_INTERVAL_MS);
    return () => clearInterval(id);
  }, [enabled, workspacePath, runIndex]);
}
