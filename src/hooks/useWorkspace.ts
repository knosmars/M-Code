import { useState, useEffect, useCallback } from 'react';
import { typedInvoke } from '../utils/ipc';
import { useToastStore } from '../stores/toastStore';
import { useT } from '../i18n/useT';

const WORKSPACE_PATHS_KEY = 'meyatu_workspace_paths';

interface IndexReport {
  file_count: number;
  languages: Record<string, number>;
  packages: Array<{ name: string }>;
  entrypoints: string[];
}

/**
 * Owns the working directory: the resolved `workspacePath`, the codebase
 * index summary injected into the system prompt, and folder selection (which
 * changes the process cwd so the AI's tools operate there).
 *
 * Extracted from ChatWindow. `selectWorkspace` reports failures through the
 * `onError` callback so the hook doesn't need the component's error state.
 */
export function useWorkspace(onError: (code: string, message: string) => void) {
  const t = useT();
  const [workspacePath, setWorkspacePath] = useState('');
  const [workspaceIndex, setWorkspaceIndex] = useState('');
  const [workspacePaths, setWorkspacePaths] = useState<string[]>(() => {
    try {
      return JSON.parse(localStorage.getItem(WORKSPACE_PATHS_KEY) ?? '[]');
    } catch {
      return [];
    }
  });

  // Index the current working directory for codebase understanding.
  const indexWorkspace = useCallback(async () => {
    try {
      const result = await typedInvoke<string>('tool_index_codebase', { path: '.' });
      const report: IndexReport = JSON.parse(result);
      const langSummary = Object.entries(report.languages)
        .map(([lang, count]) => `${lang}: ${count}`)
        .join(', ');
      const pkgSummary = report.packages.map((p) => p.name).join(', ');
      const summary = [
        `CODEBASE INDEX: ${report.file_count} files (${langSummary})`,
        pkgSummary ? `Packages: ${pkgSummary}` : '',
        report.entrypoints.length > 0 ? `Key entrypoints: ${report.entrypoints.join(', ')}` : '',
      ]
        .filter(Boolean)
        .join('\n');
      setWorkspaceIndex(summary);
    } catch {
      // Indexing is best-effort — continue without it, but surface the failure.
      useToastStore.getState().addToast('warn', t('workspace.indexFailed'));
    }
  }, [t]);

  // Change the process cwd so the AI's tools operate there, then reindex.
  const selectWorkspace = useCallback(
    async (path: string) => {
      try {
        const canonical = await typedInvoke<string>('tool_set_workspace', { path });
        setWorkspacePath(canonical);
        setWorkspacePaths((prev) => {
          const next = [canonical, ...prev.filter((x) => x !== canonical)].slice(0, 5);
          localStorage.setItem(WORKSPACE_PATHS_KEY, JSON.stringify(next));
          return next;
        });
        void indexWorkspace();
      } catch (e) {
        onError('WORKSPACE_FAILED', `切换工作目录失败：${String(e)}`);
      }
    },
    [indexWorkspace, onError],
  );

  // Remove a specific workspace path from the recent list.
  const removePath = useCallback((path: string) => {
    setWorkspacePaths((prev) => {
      const next = prev.filter((x) => x !== path).slice(0, 5);
      localStorage.setItem(WORKSPACE_PATHS_KEY, JSON.stringify(next));
      return next;
    });
  }, []);

  useEffect(() => {
    void indexWorkspace();
  }, [indexWorkspace]);

  return { workspacePath, setWorkspacePath, workspaceIndex, selectWorkspace, workspacePaths, removePath };
}
