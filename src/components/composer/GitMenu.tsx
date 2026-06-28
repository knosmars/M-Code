import { typedInvoke, normalizeError } from '../../utils/ipc';
import type { OAuthStatus } from '../../types/ipc';
import type { useGitStatus } from '../../hooks/useGitStatus';
import { useT } from '../../i18n/useT';
import shared from './composer.module.css';
import styles from './GitMenu.module.css';

type GitStatus = ReturnType<typeof useGitStatus>;

/** Git shortcuts popup (login status + git command shortcuts). */
export function GitMenu({
  gitInfo,
  ghAuth,
  isLoadingGit,
  setIsLoadingGit,
  refreshGitInfo,
  runGitCommand,
  onClose,
  onSend,
  moreOpen,
  onToggleMore,
}: {
  gitInfo: GitStatus['gitInfo'];
  ghAuth: GitStatus['ghAuth'];
  isLoadingGit: boolean;
  setIsLoadingGit: (v: boolean) => void;
  refreshGitInfo: GitStatus['refreshGitInfo'];
  runGitCommand: GitStatus['runGitCommand'];
  onClose: () => void;
  onSend: (text?: string) => void;
  moreOpen: boolean;
  onToggleMore: () => void;
}) {
  const t = useT();
  return (
    <div className={`${shared.popup} ${styles.popupGit}`}>
      <button
        type="button"
        className={shared.popupStatus}
        title={ghAuth?.loggedIn ? gitInfo?.remoteUrl ?? '' : t('git.clickLogin')}
        disabled={isLoadingGit}
        onClick={() => {
          if (ghAuth?.loggedIn && gitInfo?.remoteUrl) {
            typedInvoke<void>('tool_open_url', { url: gitInfo.remoteUrl }).catch(() => {});
          } else {
            setIsLoadingGit(true);
            // GitHub OAuth runs entirely in the backend: Rust opens the
            // browser, captures the callback, exchanges the code for a
            // token, and stores it. The client secret never reaches the
            // frontend. We just receive the final login status.
            typedInvoke<OAuthStatus>('tool_github_oauth_login')
              .then((status) => {
                if (status?.loggedIn) return refreshGitInfo();
              })
              .catch((e: unknown) => {
                onSend(t('git.loginFailed', { msg: normalizeError(e).message }));
              })
              .finally(() => setIsLoadingGit(false));
          }
        }}
      >
        {ghAuth?.loggedIn && gitInfo ? (
          <>
            <span className={`${shared.popupStatusDot} ${shared['popupStatusDot--online']}`} />
            <span className={shared.popupStatusRepo}>{gitInfo.owner}/{gitInfo.repo}</span>
            <span className={shared.popupStatusUser}>@{ghAuth.username}</span>
          </>
        ) : (
          <>
            <span className={`${shared.popupStatusDot} ${isLoadingGit ? styles['popupStatusDot--loading'] : shared['popupStatusDot--offline']}`} />
            <span className={shared.popupStatusRepo}>{isLoadingGit ? t('git.loggingIn') : t('git.notLoggedIn')}</span>
          </>
        )}
      </button>
      <div className={shared.popupDivider} />
      {([
        { label: 'Status', tool: 'tool_git_status', icon: '📊' },
        { label: 'Diff', tool: 'tool_git_diff', icon: '📝' },
        { label: 'Log', tool: 'tool_git_log', icon: '📋', extra: { n: 20 } },
      ] as { label: string; tool: string; icon: string; extra?: Record<string, number> }[]).map(({ label, tool, icon, extra }) => (
        <button
          key={label}
          type="button"
          className={shared.popupItem}
          onClick={() => runGitCommand(label, tool, extra, onClose)}
        >
          <span className={shared.popupItemIcon}>{icon}</span>
          {label}
        </button>
      ))}
      {([
        { label: 'Commit', cmd: '暂存所有变更并提交，请生成合适的提交信息', icon: '💾' },
        { label: 'Branch', cmd: '查看所有分支列表', icon: '🔀' },
        { label: 'Push', cmd: '将当前分支推送到远程仓库', icon: '⬆️' },
        { label: 'Create PR', cmd: '从当前分支创建一个 Pull Request', icon: '🔗' },
      ] as const).map(({ label, cmd, icon }) => (
        <button
          key={label}
          type="button"
          className={shared.popupItem}
          onClick={() => { onClose(); onSend(cmd); }}
        >
          <span className={shared.popupItemIcon}>{icon}</span>
          {label}
        </button>
      ))}
      <div className={shared.popupDivider} />
      <button
        type="button"
        className={shared.popupItem}
        onClick={onToggleMore}
      >
        More
        <span className={shared.popupHint}>{moreOpen ? '▾' : '▸'}</span>
      </button>
      {moreOpen && (
        <div className={shared.popupSubmenu}>
          {([
            { label: 'Diff Staged', tool: 'tool_git_diff_staged' },
          ] as const).map(({ label, tool }) => (
            <button
              key={label}
              type="button"
              className={shared.popupItem}
              onClick={() => runGitCommand(label, tool, undefined, onClose)}
            >
              {label}
            </button>
          ))}
          {([
            { label: 'Pull', cmd: '从远程仓库拉取最新代码并合并' },
            { label: 'Fetch', cmd: '获取远程仓库的最新更新' },
            { label: 'Stash', cmd: '将当前工作区的变更暂存起来' },
            { label: 'Stash Pop', cmd: '恢复最近一次 stash 的变更' },
            { label: 'Restore', cmd: '丢弃工作区的未暂存变更' },
            { label: 'Cherry-pick', cmd: '从指定提交选择性合并' },
            { label: 'Merge', cmd: '将指定分支合并到当前分支' },
          ] as const).map(({ label, cmd }) => (
            <button
              key={label}
              type="button"
              className={shared.popupItem}
              onClick={() => { onClose(); onSend(cmd); }}
            >
              {label}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
