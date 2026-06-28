import { useState, useEffect, useCallback } from 'react';
import { typedInvoke } from '../utils/ipc';
import { useToastStore } from '../stores/toastStore';
import styles from './FileTree.module.css';

interface FileNode {
  name: string;
  path: string;
  isDir: boolean;
  children: FileNode[] | null;
  expanded: boolean;
}

interface FileTreeProps {
  workspacePath: string;
  onSelectFile?: (path: string) => void;
  onOpenFile?: (path: string) => void;
}

/** Parse list_dir output into FileNode array. */
function parseEntries(path: string, output: string): FileNode[] {
  const lines = output.split('\n').filter((l) => l.length > 0);
  return lines
    .map((name): FileNode => {
      const isDir = name.endsWith('/');
      const cleanName = isDir ? name.slice(0, -1) : name;
      return {
        name: cleanName,
        path: path.endsWith('/') ? `${path}${cleanName}` : `${path}/${cleanName}`,
        isDir,
        children: null,
        expanded: false,
      };
    })
    .sort((a, b) => {
      // Directories first, then alphabetical
      if (a.isDir !== b.isDir) return a.isDir ? -1 : 1;
      return a.name.localeCompare(b.name);
    });
}

export function FileTree({ workspacePath, onSelectFile, onOpenFile }: FileTreeProps) {
  const [rootNodes, setRootNodes] = useState<FileNode[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const loadDir = useCallback(async (dirPath: string): Promise<FileNode[]> => {
    try {
      const output = await typedInvoke<string>('tool_list_dir', { path: dirPath });
      return parseEntries(dirPath, output);
    } catch (e) {
      console.error(`Failed to list directory ${dirPath}:`, e);
      useToastStore.getState().addToast('error', `目录读取失败：${dirPath}`);
      return [];
    }
  }, []);

  // Load root on mount
  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    loadDir(workspacePath).then((nodes) => {
      if (!cancelled) {
        setRootNodes(nodes);
        setLoading(false);
      }
    }).catch((e) => {
      if (!cancelled) {
        setError(String(e));
        setLoading(false);
      }
    });
    return () => { cancelled = true; };
  }, [workspacePath, loadDir]);

  const toggleExpand = useCallback(
    async (nodePath: string, _nodes: FileNode[], setNodes: React.Dispatch<React.SetStateAction<FileNode[]>>) => {
      const updateNode = (list: FileNode[]): FileNode[] =>
        list.map((n) => {
          if (n.path === nodePath) {
            if (!n.expanded && n.isDir) {
              // Expand: load children if not loaded
              if (n.children === null) {
                loadDir(nodePath).then((children) => {
                  setNodes((prev) => {
                    const patch = (items: FileNode[]): FileNode[] =>
                      items.map((x) =>
                        x.path === nodePath
                          ? { ...x, children, expanded: true }
                          : x.children !== null
                            ? { ...x, children: patch(x.children) }
                            : x,
                      );
                    return patch(prev);
                  });
                });
                // Return intermediate state showing "loading" indicator
                return { ...n, expanded: true };
              }
              return { ...n, expanded: true };
            }
            return { ...n, expanded: false };
          }
          if (n.children !== null) {
            return { ...n, children: updateNode(n.children) };
          }
          return n;
        });

      setNodes((prev) => updateNode(prev));
    },
    [loadDir],
  );

  const handleClick = useCallback(
    (node: FileNode) => {
      if (node.isDir) {
        toggleExpand(node.path, rootNodes, setRootNodes);
      } else {
        onSelectFile?.(node.path);
      }
    },
    [rootNodes, toggleExpand, onSelectFile],
  );

  const handleDoubleClick = useCallback(
    (node: FileNode) => {
      if (!node.isDir) {
        onOpenFile?.(node.path);
      }
    },
    [onOpenFile],
  );

  if (loading) {
    return (
      <div className={styles.sidebarSection}>
        <div className={styles.sidebarSectionHeader}>
          <span className={styles.sidebarSectionTitle}>Files</span>
        </div>
        <div className={styles.fileTreeLoading}>Loading…</div>
      </div>
    );
  }

  if (error) {
    return (
      <div className={styles.sidebarSection}>
        <div className={styles.sidebarSectionHeader}>
          <span className={styles.sidebarSectionTitle}>Files</span>
        </div>
        <div className={styles.fileTreeError}>{error}</div>
      </div>
    );
  }

  return (
    <div className={styles.sidebarSection}>
      <div className={styles.sidebarSectionHeader}>
        <svg viewBox="0 0 16 16" width="14" height="14" fill="none" aria-hidden="true">
          <path d="M2 4h4l2 2h6v7H2V4z" stroke="currentColor" strokeWidth="1.2" strokeLinejoin="round" />
        </svg>
        <span className={styles.sidebarSectionTitle}>Files</span>
      </div>
      <div className={styles.fileTree} role="tree" aria-label="File tree">
        {rootNodes.length === 0 ? (
          <div className={styles.fileTreeEmpty}>Empty directory</div>
        ) : (
          rootNodes.map((node) => (
            <TreeNodeRenderer
              key={node.path}
              node={node}
              depth={0}
              handleClick={handleClick}
              handleDoubleClick={handleDoubleClick}
            />
          ))
        )}
      </div>
    </div>
  );
}

/** Recursive tree node renderer extracted to avoid re-rendering full tree. */
function TreeNodeRenderer({
  node,
  depth,
  handleClick,
  handleDoubleClick,
}: {
  node: FileNode;
  depth: number;
  handleClick: (n: FileNode) => void;
  handleDoubleClick: (n: FileNode) => void;
}) {
  const paddingLeft = 12 + depth * 16;

  return (
    <div key={node.path}>
      <div
        className={`${styles.fileTreeItem}${node.isDir ? ' ' + styles['fileTreeItem--dir'] : ''}`}
        style={{ paddingLeft: `${paddingLeft}px` }}
        onClick={() => handleClick(node)}
        onDoubleClick={() => handleDoubleClick(node)}
        role="treeitem"
        aria-expanded={node.isDir ? node.expanded : undefined}
        aria-level={depth + 1}
      >
        <span className={styles.fileTreeIcon}>
          {node.isDir ? (
            node.expanded ? (
              <svg viewBox="0 0 16 16" width="14" height="14" fill="none">
                <path d="M2 5l6 4 6-4" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
              </svg>
            ) : (
              <svg viewBox="0 0 16 16" width="14" height="14" fill="none">
                <path d="M5 2l4 6-4 6" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
              </svg>
            )
          ) : (
            <svg viewBox="0 0 16 16" width="14" height="14" fill="none">
              <path d="M4 2h5l3 3v9H4V2z" stroke="currentColor" strokeWidth="1.2" strokeLinejoin="round" />
              <path d="M9 2v3h3" stroke="currentColor" strokeWidth="1.2" strokeLinejoin="round" />
            </svg>
          )}
        </span>
        <span className={`${styles.fileTreeName} truncate`}>{node.name}</span>
      </div>
      {node.expanded && node.children !== null && node.children.length > 0 && (
        <div className={styles.fileTreeChildren}>
          {node.children.map((child) => (
            <TreeNodeRenderer
              key={child.path}
              node={child}
              depth={depth + 1}
              handleClick={handleClick}
              handleDoubleClick={handleDoubleClick}
            />
          ))}
        </div>
      )}
    </div>
  );
}
