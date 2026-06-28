import { useRef, useCallback, useEffect, useState, useMemo } from 'react';
import { typedInvoke } from '../utils/ipc';
import { useGitStatus } from '../hooks/useGitStatus';
import { useSshConnection } from '../hooks/useSshConnection';
import { ImageAttachments } from './composer/ImageAttachments';
import { SlashCommandMenu } from './composer/SlashCommandMenu';
import { ModelPicker } from './composer/ModelPicker';
import { GitMenu } from './composer/GitMenu';
import { SshMenu } from './composer/SshMenu';
import { detectAtToken } from '../utils/mention';
import { useT } from '../i18n/useT';
import shared from './composer/composer.module.css';
import slash from './composer/slash.module.css';
import styles from './ChatInput.module.css';

/** An image attachment pending in the composer. */
export interface ImageAttachment {
  /** Unique client-side ID for React key + removal. */
  id: string;
  /** Base64 data URL (e.g. `data:image/png;base64,...`). */
  dataUrl: string;
  /** Original file name (if known). */
  name: string;
}

const SLASH_COMMANDS = [
  { command: '/fix', labelKey: 'slash.fix' as const, template: '/fix ' },
  { command: '/test', labelKey: 'slash.test' as const, template: '/test ' },
  { command: '/explain', labelKey: 'slash.explain' as const, template: '/explain ' },
  { command: '/refactor', labelKey: 'slash.refactor' as const, template: '/refactor ' },
  { command: '/commit', labelKey: 'slash.commit' as const, template: '/commit ' },
  { command: '/review', labelKey: 'slash.review' as const, template: '/review ' },
] as const;

interface ChatInputProps {
  /** Current input value */
  value: string;
  /** Called when the user types */
  onChange: (value: string) => void;
  /** Called when the user triggers a send (Enter key or button click).
   *  Accepts optional override text to send directly. */
  onSend: (text?: string) => void;
  /** Called when the user cancels an in-progress stream */
  onCancel?: () => void;
  /** Whether a streaming response is in progress */
  isStreaming: boolean;
  /** Available models to choose from */
  models: string[];
  /** Currently selected model */
  selectedModel: string | null;
  /** Called when user picks a different model */
  onModelChange: (model: string) => void;
  /** Number of available tools */
  toolCount?: number;
  /** Status message shown above the input area */
  providerStatus?: string;
  /** Whether the input should be disabled entirely */
  disabled?: boolean;
  /** Whether the app is offline; disables sending and shows an offline hint. */
  isOffline?: boolean;
  /** Workspace / context folder label shown as a pill */
  contextLabel?: string;
  /** Placeholder text for the textarea */
  placeholder?: string;
  /** Whether the conversation already has messages */
  inSession?: boolean;
  /** Called when the context/folder pill is clicked */
  onContextClick?: () => void;
  /** Called when the bottom "+" is clicked — attach a local file for the AI */
  onAttachFile?: () => void;
  /** Pending image attachments to display as thumbnails. */
  imageAttachments?: ImageAttachment[];
  /** Called when user adds image(s) via paste, drag-drop, or file picker. */
  onAddImages?: (images: ImageAttachment[]) => void;
  /** Called when user removes an image attachment by ID. */
  onRemoveImage?: (id: string) => void;
  /** Current permission mode */
  permissionMode?: 'ask' | 'accept-edits' | 'plan' | 'auto';
  /** Called when user selects a permission mode */
  onSelectPermissionMode?: (mode: 'ask' | 'accept-edits' | 'plan' | 'auto') => void;
  /** Total context tokens for the current session */
  contextTokens?: number;
  /** Called when user selects a context type (Local or SSH) */
  onSelectContextType?: (type: 'local' | 'ssh') => void;
  /** Active workspace mode (local vs ssh) — highlighted in the Local menu. */
  contextType?: 'local' | 'ssh';
  workspacePath?: string;
  /** Previously opened workspace paths, for quick switching. */
  workspacePaths?: string[];
  /** Called when user wants to remove a workspace path from recents. */
  onRemoveWorkspacePath?: (path: string) => void;
  /** Text of a pending 追问 (follow-up) message to show as a chip. */
  pendingInterrupt?: string | null;
  /** Called to open terminal with a specific command */
  onOpenTerminalWithCommand?: (command: string) => void;
}

const PERMISSION_LABELS: Record<'ask' | 'accept-edits' | 'plan' | 'auto', string> = {
  ask: 'Ask permissions',
  'accept-edits': 'Accept edits',
  plan: 'Plan mode',
  auto: 'Auto mode',
};

function formatContextTokens(tokens: number): string {
  if (tokens >= 1_000_000) return `${(tokens / 1_000_000).toFixed(1)}m tokens`;
  if (tokens >= 1_000) return `${(tokens / 1_000).toFixed(1)}k tokens`;
  return `${tokens} tokens`;
}

/** Cancel (stop square) icon. */
function StopIcon() {
  return (
    <svg viewBox="0 0 16 16" fill="none" aria-hidden="true">
      <rect x="3.5" y="3.5" width="9" height="9" rx="1.5" stroke="currentColor" strokeWidth="1.4" />
    </svg>
  );
}

/** Send (arrow up) icon. */
function SendIcon() {
  return (
    <svg viewBox="0 0 16 16" fill="none" aria-hidden="true">
      <path d="M8 13V4M4.5 7.5L8 4l3.5 3.5" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

export function ChatInput({
  value,
  onChange,
  onSend,
  onCancel,
  isStreaming,
  models,
  selectedModel,
  onModelChange,
  providerStatus,
  disabled = false,
  isOffline = false,
  contextLabel = 'workspace',
  placeholder,
  inSession = false,
  onContextClick,
  imageAttachments = [],
  onAddImages,
  onRemoveImage,
  permissionMode = 'ask',
  onSelectPermissionMode,
  contextTokens = 0,
  onSelectContextType,
  contextType = 'local',
  workspacePath = '.',
  workspacePaths = [],
  onRemoveWorkspacePath,
  pendingInterrupt = null,
}: ChatInputProps) {
  const t = useT();
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const composerRef = useRef<HTMLDivElement>(null);
  const [menuOpen, setMenuOpen] = useState<string | null>(null);
  const [modelMoreOpen, setModelMoreOpen] = useState(false);
  const [gitMoreOpen, setGitMoreOpen] = useState(false);
  const [slashOpen, setSlashOpen] = useState(false);
  const [slashIndex, setSlashIndex] = useState(0);
  // History of sent messages — persists via ref per session.
  const historyCache = useRef<string[]>([]);
  const historyIdxRef = useRef(-1);
  // Snapshot of the current input when user starts browsing history.
  const currentDraftRef = useRef('');

  // Git/GitHub status + SSH connection each live in a dedicated hook.
  const { gitInfo, ghAuth, isLoadingGit, setIsLoadingGit, refreshGitInfo, runGitCommand } =
    useGitStatus(workspacePath, onSend);
  const {
    sshMoreOpen, setSshMoreOpen,
    sshConnected, sshHost,
    sshConnectOpen, setSshConnectOpen,
    sshForm, setSshForm,
    sshAuthMode, setSshAuthMode,
    sshConnecting,
    sshError, setSshError,
    handleSshConnect, handleSshDisconnect, runSshCommand,
  } = useSshConnection(onSend);
  // ---- @file mention menu ----
  const [atOpen, setAtOpen] = useState(false);
  const [atIndex, setAtIndex] = useState(0);
  const [atQuery, setAtQuery] = useState('');
  const [fileList, setFileList] = useState<string[]>([]);

  const [dragOver, setDragOver] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);

  /** Max dimension (px) for image compression before sending. */
  const MAX_IMG_DIM = 1024;
  /** JPEG quality for compression (0–1). */
  const IMG_QUALITY = 0.8;

  const fileToImageAttachment = useCallback((file: File): Promise<ImageAttachment | null> => {
    if (!file.type.startsWith('image/')) return Promise.resolve(null);
    return new Promise((resolve) => {
      const img = new Image();
      const objectUrl = URL.createObjectURL(file);
      img.onload = () => {
        URL.revokeObjectURL(objectUrl);
        let { width, height } = img;
        if (width > MAX_IMG_DIM || height > MAX_IMG_DIM) {
          const scale = MAX_IMG_DIM / Math.max(width, height);
          width = Math.round(width * scale);
          height = Math.round(height * scale);
        }
        const canvas = document.createElement('canvas');
        canvas.width = width;
        canvas.height = height;
        const ctx = canvas.getContext('2d');
        if (!ctx) {
          const reader = new FileReader();
          reader.onload = () => resolve({
            id: crypto.randomUUID(),
            dataUrl: reader.result as string,
            name: file.name,
          });
          reader.onerror = () => resolve(null);
          reader.readAsDataURL(file);
          return;
        }
        ctx.drawImage(img, 0, 0, width, height);
        const dataUrl = canvas.toDataURL('image/jpeg', IMG_QUALITY);
        resolve({ id: crypto.randomUUID(), dataUrl, name: file.name });
      };
      img.onerror = () => {
        URL.revokeObjectURL(objectUrl);
        resolve(null);
      };
      img.src = objectUrl;
    });
  }, []);

  const handlePaste = useCallback(async (e: React.ClipboardEvent) => {
    if (!onAddImages) return;
    const items = Array.from(e.clipboardData.items);
    const imageFiles = items
      .filter((item) => item.kind === 'file' && item.type.startsWith('image/'))
      .map((item) => item.getAsFile())
      .filter((f): f is File => f !== null);
    if (imageFiles.length === 0) return;
    e.preventDefault();
    const results = await Promise.all(imageFiles.map(fileToImageAttachment));
    const valid = results.filter((r): r is ImageAttachment => r !== null);
    if (valid.length > 0) onAddImages(valid);
  }, [onAddImages, fileToImageAttachment]);

  const handleDragOver = useCallback((e: React.DragEvent) => {
    if (!onAddImages) return;
    const hasImage = Array.from(e.dataTransfer.types).includes('Files');
    if (!hasImage) return;
    e.preventDefault();
    setDragOver(true);
  }, [onAddImages]);

  const handleDragLeave = useCallback(() => setDragOver(false), []);

  const handleDrop = useCallback(async (e: React.DragEvent) => {
    if (!onAddImages) return;
    e.preventDefault();
    setDragOver(false);
    const files = Array.from(e.dataTransfer.files).filter((f) => f.type.startsWith('image/'));
    if (files.length === 0) return;
    const results = await Promise.all(files.map(fileToImageAttachment));
    const valid = results.filter((r): r is ImageAttachment => r !== null);
    if (valid.length > 0) onAddImages(valid);
  }, [onAddImages, fileToImageAttachment]);

  const handleImagePickerClick = useCallback(() => {
    fileInputRef.current?.click();
  }, []);

  const handleImageFileChange = useCallback(async (e: React.ChangeEvent<HTMLInputElement>) => {
    if (!onAddImages) return;
    const files = Array.from(e.target.files ?? []).filter((f) => f.type.startsWith('image/'));
    if (files.length === 0) return;
    const results = await Promise.all(files.map(fileToImageAttachment));
    const valid = results.filter((r): r is ImageAttachment => r !== null);
    if (valid.length > 0) onAddImages(valid);
    if (fileInputRef.current) fileInputRef.current.value = '';
  }, [onAddImages, fileToImageAttachment]);

  const slashFiltered = useMemo(() => {
    if (!value.startsWith('/')) return SLASH_COMMANDS;
    const q = value.split(/\s/)[0].toLowerCase();
    const matches = SLASH_COMMANDS.filter(c => c.command.startsWith(q));
    return matches.length > 0 ? matches : SLASH_COMMANDS;
  }, [value]);

  const trimValue = value.trim();
  const hasContent = trimValue.length > 0 || imageAttachments.length > 0;
  const isBlocked = disabled || isOffline;
  const canSend = hasContent && !isBlocked;
  // When streaming, allow the user to interrupt with a new message ("追问").
  const canInterrupt = hasContent && isStreaming && !isBlocked;
  const currentModel = selectedModel ?? models[0] ?? '';

  const resize = useCallback(() => {
    const el = textareaRef.current;
    if (el === null) return;
    el.style.height = 'auto';
    el.style.height = `${Math.min(el.scrollHeight, 180)}px`;
  }, []);

  useEffect(() => {
    resize();
  }, [value, resize]);

  useEffect(() => {
    if (menuOpen === null) return;
    const handleClick = (e: MouseEvent) => {
      if (composerRef.current && !composerRef.current.contains(e.target as Node)) {
        setMenuOpen(null);
        setModelMoreOpen(false);
        setGitMoreOpen(false);
      }
    };
    document.addEventListener('mousedown', handleClick);
    return () => document.removeEventListener('mousedown', handleClick);
  }, [menuOpen]);

  useEffect(() => {
    if (!slashOpen) return;
    const handleClick = (e: MouseEvent) => {
      if (composerRef.current && !composerRef.current.contains(e.target as Node)) {
        setSlashOpen(false);
      }
    };
    document.addEventListener('mousedown', handleClick);
    return () => document.removeEventListener('mousedown', handleClick);
  }, [slashOpen]);

  useEffect(() => {
    const shouldShow = value.startsWith('/') && !value.includes('\n');
    setSlashOpen(shouldShow);
    if (shouldShow) setSlashIndex(0);
  }, [value]);

  const selectSlashCommand = useCallback((idx: number) => {
    const cmd = slashFiltered[idx];
    if (!cmd) return;
    onChange(cmd.template);
    setSlashOpen(false);
    setTimeout(() => textareaRef.current?.focus(), 0);
  }, [slashFiltered, onChange]);

  // ---- @file mention: filter, load, select ----
  const fileFiltered = useMemo(() => {
    const q = atQuery.toLowerCase();
    const matched = q ? fileList.filter((f) => f.toLowerCase().includes(q)) : fileList;
    return matched.slice(0, 8);
  }, [fileList, atQuery]);

  // Detect an @file token at the caret on every input change.
  const handleChange = useCallback((e: React.ChangeEvent<HTMLTextAreaElement>) => {
    const v = e.target.value;
    onChange(v);
    const caret = e.target.selectionStart ?? v.length;
    const token = detectAtToken(v, caret);
    if (token) {
      setAtQuery(token.query);
      setAtIndex(0);
      setAtOpen(true);
    } else {
      setAtOpen(false);
    }
  }, [onChange]);

  // Lazily fetch the workspace file list the first time the menu opens.
  useEffect(() => {
    if (!atOpen || fileList.length > 0) return;
    let cancelled = false;
    typedInvoke<string>('tool_glob', { pattern: '**/*', path: workspacePath })
      .then((out) => {
        if (cancelled) return;
        const all = out ? out.split('\n').filter(Boolean) : [];
        // Drop dependency/build noise so the picker stays useful.
        const cleaned = all.filter(
          (f) => !/(^|\/)(node_modules|target|dist|build|\.git|\.next|vendor)(\/|$)/.test(f),
        );
        setFileList(cleaned);
      })
      .catch(() => {});
    return () => { cancelled = true; };
  }, [atOpen, fileList.length, workspacePath]);

  // Invalidate the cached file list when the workspace changes.
  useEffect(() => { setFileList([]); }, [workspacePath]);

  // Close the @menu on outside click.
  useEffect(() => {
    if (!atOpen) return;
    const handleClick = (e: MouseEvent) => {
      if (composerRef.current && !composerRef.current.contains(e.target as Node)) {
        setAtOpen(false);
      }
    };
    document.addEventListener('mousedown', handleClick);
    return () => document.removeEventListener('mousedown', handleClick);
  }, [atOpen]);

  const selectFile = useCallback((idx: number) => {
    const file = fileFiltered[idx];
    if (!file) return;
    const el = textareaRef.current;
    const caret = el?.selectionStart ?? value.length;
    const token = detectAtToken(value, caret);
    if (!token) return;
    const insert = `@${file} `;
    const before = value.slice(0, token.start);
    const after = value.slice(caret);
    onChange(`${before}${insert}${after}`);
    setAtOpen(false);
    const pos = before.length + insert.length;
    setTimeout(() => {
      const t2 = textareaRef.current;
      if (t2) { t2.focus(); t2.setSelectionRange(pos, pos); }
    }, 0);
  }, [fileFiltered, value, onChange]);

  const handleKeyDown = useCallback(
    (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
      if (atOpen && fileFiltered.length > 0) {
        if (event.key === 'ArrowDown') {
          event.preventDefault();
          setAtIndex(i => (i + 1) % fileFiltered.length);
          return;
        }
        if (event.key === 'ArrowUp') {
          event.preventDefault();
          setAtIndex(i => (i - 1 + fileFiltered.length) % fileFiltered.length);
          return;
        }
        if (event.key === 'Enter' || event.key === 'Tab') {
          event.preventDefault();
          selectFile(atIndex);
          return;
        }
        if (event.key === 'Escape') {
          event.preventDefault();
          setAtOpen(false);
          return;
        }
      }
      if (slashOpen && slashFiltered.length > 0) {
        if (event.key === 'ArrowDown') {
          event.preventDefault();
          setSlashIndex(i => (i + 1) % slashFiltered.length);
          return;
        }
        if (event.key === 'ArrowUp') {
          event.preventDefault();
          setSlashIndex(i => (i - 1 + slashFiltered.length) % slashFiltered.length);
          return;
        }
        if (event.key === 'Enter' || event.key === 'Tab') {
          event.preventDefault();
          selectSlashCommand(slashIndex);
          return;
        }
        if (event.key === 'Escape') {
          event.preventDefault();
          setSlashOpen(false);
          return;
        }
      }
      if (event.key === 'Enter' && !event.shiftKey) {
        event.preventDefault();
        // Save current text to history before sending
        if (canSend || canInterrupt) {
          const trimmed = value.trim();
          if (trimmed.length > 0) {
            historyCache.current = [trimmed, ...historyCache.current.filter((h) => h !== trimmed)].slice(0, 50);
          }
          historyIdxRef.current = -1;
          onSend();
        }
      }
      // ArrowUp — browse input history (newest → oldest)
      if (event.key === 'ArrowUp' && !event.shiftKey && !event.ctrlKey && !slashOpen && !atOpen && textareaRef.current) {
        const history = historyCache.current;
        if (history.length === 0) return;
        event.preventDefault();
        if (historyIdxRef.current === -1) currentDraftRef.current = value; // save draft
        historyIdxRef.current = Math.min(historyIdxRef.current + 1, history.length - 1);
        onChange(history[historyIdxRef.current]);
      }
      // ArrowDown — go back to newer history or restore draft
      if (event.key === 'ArrowDown' && historyIdxRef.current >= 0 && !slashOpen && !atOpen) {
        event.preventDefault();
        historyIdxRef.current -= 1;
        if (historyIdxRef.current < 0) {
          historyIdxRef.current = -1;
          onChange(currentDraftRef.current);
        } else {
          onChange(historyCache.current[historyIdxRef.current]);
        }
      }
      // Tab+Shift cycles permission modes
      if (event.key === 'Tab' && event.shiftKey) {
        event.preventDefault();
        const MODES: Array<'ask' | 'accept-edits' | 'plan' | 'auto'> = ['ask', 'accept-edits', 'plan', 'auto'];
        const idx = MODES.indexOf(permissionMode);
        const next = MODES[(idx + 1) % MODES.length];
        onSelectPermissionMode?.(next);
      }
    },
    [canSend, canInterrupt, onSend, onCancel, slashOpen, slashFiltered, slashIndex, selectSlashCommand,
     atOpen, fileFiltered, atIndex, selectFile, permissionMode, onSelectPermissionMode],
  );

  const ph = placeholder ?? (inSession ? t('chat.placeholder') : 'Describe a task or ask a question');

  const toggleMenu = (id: string) => setMenuOpen(menuOpen === id ? null : id);
  const closeMenu = () => { setMenuOpen(null); setModelMoreOpen(false); setGitMoreOpen(false); setSshMoreOpen(false); setSshConnectOpen(false); setSshError(''); };

  return (
    <div
      className={`${styles.container}${dragOver ? ` ${styles.dragOver}` : ''}`}
      ref={composerRef}
      onDragOver={handleDragOver}
      onDragLeave={handleDragLeave}
      onDrop={handleDrop}
    >
      {isOffline ? (
        <div className={`${styles.status} ${styles['status--offline']}`}>{t('composer.offline')}</div>
      ) : (
        providerStatus && <div className={styles.status}>{providerStatus}</div>
      )}

      <input
        ref={fileInputRef}
        type="file"
        accept="image/*"
        multiple
        style={{ display: 'none' }}
        onChange={handleImageFileChange}
      />

      <div className={styles.composer}>
        {imageAttachments.length > 0 && (
          <ImageAttachments images={imageAttachments} onRemove={onRemoveImage} />
        )}

        <div className={styles.pills}>
          <button
            type="button"
            className={`${styles.pill} ${styles['pill--button']}`}
            onClick={() => toggleMenu('local')}
            title="Connect workspace"
          >
            <svg viewBox="0 0 16 16" width="12" height="12" fill="none" aria-hidden="true">
              <rect x="2" y="3" width="12" height="8" rx="1" stroke="currentColor" strokeWidth="1.2" />
              <path d="M5.5 13.5h5" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" />
            </svg>
            Local
          </button>

          {menuOpen === 'local' && (
            <div className={shared.popup} style={{ left: 0 }}>
              {/* Mode only — picking the actual folder is done via the "+" button. */}
              <div className={styles.popupHeading}>{t('composer.workspaceMode')}</div>
              <button type="button"
                className={`${shared.popupItem}${contextType === 'local' ? ` ${shared['popupItem--active']}` : ''}`}
                onClick={() => { onSelectContextType?.('local'); closeMenu(); }}>
                <svg viewBox="0 0 16 16" width="13" height="13" fill="none" aria-hidden="true">
                  <rect x="2" y="3" width="12" height="8" rx="1" stroke="currentColor" strokeWidth="1.2" />
                  <path d="M5.5 13.5h5" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" />
                </svg>
                {t('composer.modeLocal')}
              </button>
              <button type="button"
                className={`${shared.popupItem}${contextType === 'ssh' ? ` ${shared['popupItem--active']}` : ''}`}
                onClick={() => { onSelectContextType?.('ssh'); closeMenu(); }}>
                <svg viewBox="0 0 16 16" width="13" height="13" fill="none" aria-hidden="true">
                  <path d="M2.5 10.5l2.5-2.5-2.5-2.5M6.5 11.5h7" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" strokeLinejoin="round" />
                </svg>
                {t('composer.modeSshServer')}
              </button>
              {workspacePaths.length > 0 && (
                <>
                  <div className={styles.popupHeading}>Recent</div>
                  {workspacePaths.map((p) => (
                    <button key={p} type="button" className={shared.popupItem}
                      onClick={() => { onSelectContextType?.('local'); closeMenu(); }}
                      title={p}>
                      <span style={{ flex: 1, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
                        {p.split(/[\\/]/).filter(Boolean).pop() ?? p}
                      </span>
                      <button
                        type="button"
                        className={styles.workspaceDelete}
                        onClick={(e) => { e.stopPropagation(); onRemoveWorkspacePath?.(p); }}
                        aria-label={`Remove ${p}`}
                        title="Remove from list"
                      >×</button>
                    </button>
                  ))}
                </>
              )}
            </div>
          )}

          <button
            type="button"
            className={`${styles.pill} ${styles['pill--button']}`}
            onClick={onContextClick}
            title="Browse project files"
          >
            <svg viewBox="0 0 16 16" width="12" height="12" fill="none" aria-hidden="true">
              <path d="M2 4.5h4l1.2 1.5H14v6.5H2V4.5z" stroke="currentColor" strokeWidth="1.2" strokeLinejoin="round" />
            </svg>
            {contextLabel}
          </button>

          <div className={styles.popupAnchor}>
            <button
              type="button"
              className={`${styles.pill} ${styles['pill--button']} ${styles['pill--git']}`}
              onClick={() => toggleMenu('git')}
              title="Git shortcuts"
            >
              <svg viewBox="0 0 16 16" width="12" height="12" fill="none" aria-hidden="true">
                <circle cx="4" cy="8" r="2" stroke="currentColor" strokeWidth="1.2" />
                <circle cx="12" cy="4" r="2" stroke="currentColor" strokeWidth="1.2" />
                <circle cx="12" cy="12" r="2" stroke="currentColor" strokeWidth="1.2" />
                <path d="M6 8h4M12 6v4" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" />
              </svg>
              Git
            </button>
            {menuOpen === 'git' && (
              <GitMenu
                gitInfo={gitInfo}
                ghAuth={ghAuth}
                isLoadingGit={isLoadingGit}
                setIsLoadingGit={setIsLoadingGit}
                refreshGitInfo={refreshGitInfo}
                runGitCommand={runGitCommand}
                onClose={closeMenu}
                onSend={onSend}
                moreOpen={gitMoreOpen}
                onToggleMore={() => setGitMoreOpen((v) => !v)}
              />
            )}
          </div>
          <div className={styles.popupAnchor}>
            <button
              type="button"
              className={`${styles.pill} ${styles['pill--button']} ${styles['pill--ssh']}`}
              onClick={() => toggleMenu('ssh')}
              title="SSH shortcuts"
            >
              <svg viewBox="0 0 16 16" width="12" height="12" fill="none" aria-hidden="true">
                <rect x="2" y="3" width="12" height="10" rx="1.5" stroke="currentColor" strokeWidth="1.2" />
                <path d="M5 7.5l2 1.5-2 1.5" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" strokeLinejoin="round" />
                <path d="M8 10.5h3" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" />
              </svg>
              SSH
            </button>
            {menuOpen === 'ssh' && (
              <SshMenu
                sshConnected={sshConnected}
                sshHost={sshHost}
                handleSshDisconnect={handleSshDisconnect}
                sshConnectOpen={sshConnectOpen}
                setSshConnectOpen={setSshConnectOpen}
                sshForm={sshForm}
                setSshForm={setSshForm}
                handleSshConnect={handleSshConnect}
                sshConnecting={sshConnecting}
                sshAuthMode={sshAuthMode}
                setSshAuthMode={setSshAuthMode}
                sshError={sshError}
                runSshCommand={runSshCommand}
                closeMenu={closeMenu}
                sshMoreOpen={sshMoreOpen}
                setSshMoreOpen={setSshMoreOpen}
                onSend={onSend}
              />
            )}
          </div>
          {pendingInterrupt && (
            <span className={`${styles.pill} ${styles['pill--interrupt']}`}>
              ⏳ 等候执行：{pendingInterrupt}
            </span>
          )}
        </div>

        <div className={styles.box}>
          {slashOpen && slashFiltered.length > 0 && (
            <SlashCommandMenu
              items={slashFiltered}
              activeIndex={slashIndex}
              onSelect={selectSlashCommand}
              onHover={setSlashIndex}
              t={t}
            />
          )}
          {atOpen && fileFiltered.length > 0 && (
            <div className={slash.slashMenu}>
              {fileFiltered.map((file, i) => (
                <button
                  key={file}
                  type="button"
                  className={`${slash.slashMenuItem}${i === atIndex ? ` ${slash['slashMenuItem--active']}` : ''}`}
                  onMouseDown={(e) => { e.preventDefault(); selectFile(i); }}
                  onMouseEnter={() => setAtIndex(i)}
                >
                  <span className={slash.slashMenuCommand}>@{file.split('/').pop()}</span>
                  <span className={slash.slashMenuDesc}>{file}</span>
                </button>
              ))}
            </div>
          )}
          <textarea
            ref={textareaRef}
            value={value}
            onChange={handleChange}
            onKeyDown={handleKeyDown}
            onPaste={handlePaste}
            placeholder={ph}
            rows={1}
            disabled={disabled}
            aria-label="Chat message input"
          />
          {isStreaming ? (
            <button
              type="button"
              className={`${styles.send} ${styles['send--stop']}`}
              onClick={onCancel}
              aria-label="Stop"
              title="Stop (Esc)"
            >
              <StopIcon />
            </button>
          ) : (
            <button
              type="button"
              className={styles.send}
              onClick={() => onSend()}
              disabled={!canSend}
              aria-label="Send message"
              title="Send (Enter)"
            >
              <SendIcon />
            </button>
          )}
        </div>

        <div className={styles.footer}>
          <div className={styles.footerLeft}>
            <div className={styles.popupAnchor}>
              <button
                type="button"
                className={`${styles.mode} ${styles[`mode--${permissionMode}`]}`}
                onClick={() => toggleMenu('mode')}
                title="Change permission mode"
              >
                <span className={styles.modeDot} />
                {PERMISSION_LABELS[permissionMode]}
              </button>
              {menuOpen === 'mode' && (
                <div className={`${shared.popup} ${styles.popupLeft}`}>
                  {(Object.keys(PERMISSION_LABELS) as Array<'ask' | 'accept-edits' | 'plan' | 'auto'>).map((mode) => (
                    <button key={mode} type="button"
                      className={`${shared.popupItem}${permissionMode === mode ? ` ${shared['popupItem--active']}` : ''}`}
                      onClick={() => { onSelectPermissionMode?.(mode); closeMenu(); }}>
                      <span className={styles.modeDot} />
                      {PERMISSION_LABELS[mode]}
                    </button>
                  ))}
                </div>
              )}
            </div>
            <button type="button" className={styles.add} aria-label="Attach image" title="Attach an image (paste, drag-drop, or pick)" onClick={handleImagePickerClick}>
              <svg viewBox="0 0 16 16" width="12" height="12" fill="none" aria-hidden="true">
                <path d="M8 3v10M3 8h10" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" />
              </svg>
            </button>
          </div>

          <div className={styles.footerRight}>
            {models.length > 0 && (
              <div className={`${styles.popupAnchor} ${styles.model}`}>
                <button
                  type="button"
                  className={styles.modelBtn}
                  onClick={() => toggleMenu('model')}
                  disabled={disabled || isStreaming}
                  title="Select model"
                >
                  {currentModel || 'model'}
                </button>
                {menuOpen === 'model' && (
                  <ModelPicker
                    models={models}
                    currentModel={currentModel}
                    onSelect={onModelChange}
                    onClose={closeMenu}
                    moreOpen={modelMoreOpen}
                    onToggleMore={() => setModelMoreOpen((v) => !v)}
                  />
                )}
              </div>
            )}
            {contextTokens > 0 && (
              <span className={styles.contextTokens} title={t('composer.contextLengthTitle')}>
                {formatContextTokens(contextTokens)}
              </span>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
