import { useState, useEffect, useCallback } from 'react';
import { typedInvoke } from '../utils/ipc';
import type { GitRemoteInfo, GhAuthStatus } from '../types/ipc';

/**
 * Owns the composer's Git/GitHub status: remote info, gh auth state, and
 * running git tools. Refreshes whenever the workspace changes.
 *
 * Extracted from ChatInput; the JSX consumes the returned values under their
 * original names so the markup is unchanged. `runGitCommand` takes an optional
 * `onDone` callback (the call site passes `closeMenu`) to keep the hook free of
 * the composer's menu coordination.
 */
export function useGitStatus(workspacePath: string, onSend: (text: string) => void) {
  const [gitInfo, setGitInfo] = useState<GitRemoteInfo | null>(null);
  const [ghAuth, setGhAuth] = useState<GhAuthStatus | null>(null);
  const [isLoadingGit, setIsLoadingGit] = useState(false);

  const refreshGitInfo = useCallback(() => {
    if (!workspacePath || workspacePath === '.') {
      setGitInfo(null);
      setGhAuth(null);
      return;
    }
    typedInvoke<GitRemoteInfo>('tool_git_remote_info', { path: workspacePath })
      .then(setGitInfo)
      .catch(() => setGitInfo(null));
    typedInvoke<GhAuthStatus>('tool_gh_auth_status', { path: workspacePath })
      .then(setGhAuth)
      .catch(() => setGhAuth(null));
  }, [workspacePath]);

  useEffect(() => {
    refreshGitInfo();
  }, [refreshGitInfo]);

  const runGitCommand = useCallback(
    async (label: string, toolName: string, extraArgs?: Record<string, unknown>, onDone?: () => void) => {
      try {
        const result = await typedInvoke<string>(toolName, { path: workspacePath, ...extraArgs });
        onSend(`Git ${label}:\n\`\`\`\n${result}\n\`\`\`\n\n请分析以上 ${label} 结果并帮助处理。`);
      } catch (e) {
        onSend(`Git ${label} 失败: ${String(e)}\n\n请检查 Git 配置。`);
      }
      onDone?.();
    },
    [workspacePath, onSend],
  );

  return { gitInfo, ghAuth, isLoadingGit, setIsLoadingGit, refreshGitInfo, runGitCommand };
}
