import {
  useState,
  useRef,
  useCallback,
  useEffect,
  useMemo,
} from 'react';
import type { Message } from '../types/message';
import type { ToolCall } from '../types/message';
import { REGISTRY_DEFS } from '../agent/toolRegistry';
import { useSessionStore } from '../stores/sessionStore';
import { useProviderStore } from '../stores/providerStore';
import { useSettingsStore } from '../stores/settingsStore';
import { useT } from '../i18n/useT';
import type { TranslationKey } from '../i18n/translations';
import { useViewStore } from '../stores/viewStore';
import { loadDraft, saveDraft, NEW_SESSION_DRAFT_KEY } from '../utils/draftStore';
import { typedInvoke, normalizeError } from '../utils/ipc';
import { useWorkspace } from '../hooks/useWorkspace';
import { useAutoIndex } from '../hooks/useAutoIndex';
import { useOnlineStatus } from '../hooks/useOnlineStatus';
import { useAgentSession, type UseAgentSession } from '../hooks/useAgentSession';
import { MessageBubble } from './MessageBubble';
import { ChatInput, type ImageAttachment } from './ChatInput';
import { ParallelAgentsCard } from './ParallelAgentsCard';
import { AgentsPanel } from './AgentsPanel';
import { WelcomeScreen } from './WelcomeScreen';
import { TitleBar } from './TitleBar';
import { FileTree } from './FileTree';
import { CodeEditor } from './CodeEditor';
import { ErrorBoundary } from './ErrorBoundary';
import { PermissionDialog } from './PermissionDialog';
import { SessionList } from './SessionList';
import { CommandPalette } from './CommandPalette';
import { BrandIcon } from './BrandIcon';
import { ThinkingIndicator } from './ThinkingIndicator';
import { NewSessionDialog } from './NewSessionDialog';
import { TokenDashboard } from './TokenDashboard';
import { Terminal } from './Terminal';
import { FileSyncNotifications } from './FileSyncNotifications';
import { Toaster } from './Toaster';
import codeEditorStyles from './CodeEditor.module.css';
import rightPanelStyles from './RightPanel.module.css';
import terminalStyles from './Terminal.module.css';
import styles from './ChatWindow.module.css';

interface OpenFile {
  path: string;
  content: string | null;
}

/** Backend error code → translation key for the error banner. OFFLINE reuses composer.offline. */
const ERROR_LABEL_KEYS: Record<string, TranslationKey> = {
  rate_limited: 'chat.error.rateLimited',
  provider: 'chat.error.provider',
  http: 'chat.error.http',
  keychain: 'chat.error.keychain',
  not_found: 'chat.error.notFound',
  permission_denied: 'chat.error.permissionDenied',
  serialization: 'chat.error.serialization',
  internal: 'chat.error.internal',
  NO_API_KEY: 'chat.error.noApiKey',
  VALIDATION_ERROR: 'chat.error.validation',
  SESSION_LIMIT: 'chat.error.sessionLimit',
  IPC_ERROR: 'chat.error.ipc',
  STREAM_TIMEOUT: 'chat.error.streamTimeout',
  OFFLINE: 'composer.offline',
};

function errorLabel(code: string, t: (k: TranslationKey) => string): string {
  const key = ERROR_LABEL_KEYS[code];
  return key ? t(key) : code;
}

/** Create an ephemeral Message-like object for streaming display. */
function ephemeralMessage(
  id: string,
  content: string,
  toolCalls: ToolCall[],
): Message {
  return {
    id,
    role: 'assistant',
    content,
    toolCalls: toolCalls.length > 0 ? toolCalls : undefined,
    timestamp: Date.now(),
  };
}

/**
 * Main chat interface for Meyatu Code.
 *
 * Orchestrates the chat flow:
 *   user input -> AgentLoop -> streaming events -> UI updates -> store persistence
 *
 * Layout: sidebar (sessions) + main area (messages + input).
 */
export function ChatWindow() {
  // ---- Zustand stores ----
  const {
    sessions,
    currentSessionId,
    loadSessions,
    createSession,
    deleteSession,
    renameSession,
    setCurrentSession,
    getCurrentSession,
  } = useSessionStore();
  const navigate = useViewStore((s) => s.navigate);

  const { providers, activeProviderId, selectedModel, setSelectedModel, setActiveProvider } = useProviderStore();
  const showTimestamps = useSettingsStore((s) => s.showTimestamps);
  const online = useOnlineStatus();
  const t = useT();

  // ---- Input state ----
  const [inputText, setInputText] = useState('');

  // ---- Per-session draft persistence (survives restart / session switch) ----
  // Tracks which session the current `inputText` belongs to, so the persist
  // effect writes under the right key even mid-switch.
  const draftSidRef = useRef<string>(currentSessionId ?? NEW_SESSION_DRAFT_KEY);
  // Restore the saved draft whenever the active session changes (and on mount).
  useEffect(() => {
    const sid = currentSessionId ?? NEW_SESSION_DRAFT_KEY;
    draftSidRef.current = sid;
    setInputText(loadDraft(sid));
  }, [currentSessionId]);
  // Persist the draft on every input change (cheap localStorage write).
  useEffect(() => {
    saveDraft(draftSidRef.current, inputText);
  }, [inputText]);

  const [searchFocusKey, setSearchFocusKey] = useState(0);
  const [commandPaletteOpen, setCommandPaletteOpen] = useState(false);
  const [detailsOpen, setDetailsOpen] = useState(false);
  const [terminalOpen, setTerminalOpen] = useState(false);
  const terminalSessionIdRef = useRef<string>('');
  const terminalInitialCommandRef = useRef<string>('');
  const [sidebarOpen, setSidebarOpen] = useState(true);
  const [sidebarPane, setSidebarPane] = useState<"sessions" | "files">("sessions");
  const [gatewayOpen, setGatewayOpen] = useState(false);
  const [newSessionDialogOpen, setNewSessionDialogOpen] = useState(false);
  const [showWelcome, setShowWelcome] = useState(false);

  // Session title inline editing + recent sessions dropdown
  const [editingTitle, setEditingTitle] = useState<string | null>(null);
  const [sessionDropdownOpen, setSessionDropdownOpen] = useState(false);
  const titleInputRef = useRef<HTMLInputElement>(null);

  // Workspace mode (local vs ssh) — set via the Local menu; folder selection
  // is separate (the "+" button), per the user's design.
  const [contextType, setContextType] = useState<'local' | 'ssh'>('local');
  const [imageAttachments, setImageAttachments] = useState<ImageAttachment[]>([]);

  // Close session dropdown on outside click
  useEffect(() => {
    if (!sessionDropdownOpen) return;
    const handler = (e: MouseEvent) => {
      const target = e.target as HTMLElement;
      if (!target.closest(`.${styles.sessionHeaderDropdown}`) && !target.closest(`.${styles.sessionHeaderCrumb}`)) {
        setSessionDropdownOpen(false);
      }
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [sessionDropdownOpen]);

  // ---- Workspace (cwd, codebase index, folder selection) ----
  // onWorkspaceError reports into the agent session's error banner; a ref breaks
  // the cycle (useWorkspace needs the callback before `agent` exists).
  const agentRef = useRef<UseAgentSession | null>(null);
  const onWorkspaceError = useCallback(
    (code: string, message: string) => agentRef.current?.reportError(code, message),
    [],
  );
  const { workspacePath, setWorkspacePath, workspaceIndex, selectWorkspace, workspacePaths, removePath } =
    useWorkspace(onWorkspaceError);

  useAutoIndex(workspacePath, useSettingsStore((s) => s.autoSemanticIndex));

  // ---- Derived ----
  const providersConfigured = activeProviderId !== null;

  const activeModels = useMemo(() => {
    return providers.find((p) => p.id === activeProviderId)?.models ?? providers[0]?.models ?? [];
  }, [providers, activeProviderId]);

  const effectiveModel = selectedModel ?? activeModels[0];


  // ---- Agent-run orchestration (send/stream/persist) ----
  const agent = useAgentSession({
    workspacePath,
    workspaceIndex,
    effectiveModel,
    onParallelStart: () => setDetailsOpen(true),
  });
  agentRef.current = agent;
  const {
    streamingContent, streamingMsgId, streamingSessionId, streamingToolCalls, agentBusy, error,
    thinkingStatus, thinkingTokens, elapsedSec, activeTools, planSteps, parallelRun,
    pendingInterrupt, pendingPermission, resolvePermission, permissionMode, setPermissionMode,
  } = agent;
  const dismissError = agent.dismissError;
  const handleCancel = agent.cancel;
  const handleRetry = agent.retry;
  const currentMessages: Message[] = useMemo(() => {
    const session = getCurrentSession();
    return session?.messages ?? [];
  }, [sessions, currentSessionId]);

  // Is the in-flight stream (if any) for the session currently being viewed?
  // Streaming state is global to the single agent loop, so the live bubble and
  // indicators must only render in the originating session's tab.
  const streamingForCurrent = streamingMsgId !== null && streamingSessionId === currentSessionId;

  const isEmpty = currentMessages.length === 0 && !streamingForCurrent;

  // ---- Open file tabs ----
  const [openFiles, setOpenFiles] = useState<OpenFile[]>([]);
  const [activeTab, setActiveTab] = useState<string>('chat');
  // Track open session tabs — added when user selects a session in the sidebar.
  const [openSessionTabs, setOpenSessionTabs] = useState<string[]>([]);

  const handleOpenFile = useCallback(async (path: string) => {
    // Already open — switch to it
    const existing = openFiles.find((f) => f.path === path);
    if (existing) {
      setActiveTab(path);
      return;
    }

    // Add tab with null content first, then load
    setOpenFiles((prev) => [...prev, { path, content: null }]);
    setActiveTab(path);

    try {
      const content = await typedInvoke<string>('tool_read_file', { path });
      setOpenFiles((prev) =>
        prev.map((f) => (f.path === path ? { ...f, content } : f)),
      );
    } catch (e) {
      const err = normalizeError(e);
      setOpenFiles((prev) =>
        prev.map((f) =>
          f.path === path ? { ...f, content: `Error: ${err.message}` } : f,
        ),
      );
    }
  }, [openFiles]);

  // ---- Revert a turn's file edits via its checkpoint ----
  const handleRevertCheckpoint = useCallback(async (checkpointId: string) => {
    try {
      await typedInvoke<string>('tool_checkpoint_restore', { id: checkpointId });
    } catch (e) {
      const err = normalizeError(e);
      agent.reportError(err.code === 'internal' ? 'REVERT_FAILED' : err.code, `${t('chat.error.revertFailed')}：${err.message}`);
      return;
    }
    for (const f of openFiles) {
      try {
        const content = await typedInvoke<string>('tool_read_file', { path: f.path });
        setOpenFiles((prev) => prev.map((x) => (x.path === f.path ? { ...x, content } : x)));
      } catch {
        setOpenFiles((prev) => prev.filter((x) => x.path !== f.path));
      }
    }
  }, [openFiles]);


  const handleCloseTab = useCallback((path: string) => {
    setOpenFiles((prev) => prev.filter((f) => f.path !== path));
    if (path === activeTab) {
      setActiveTab('chat');
    }
  }, [activeTab]);

  const handleCloseSessionTab = useCallback((id: string) => {
    const idx = openSessionTabs.indexOf(id);
    const next = openSessionTabs.filter((sid) => sid !== id);
    setOpenSessionTabs(next);
    // Only re-navigate when closing the session currently being viewed; closing
    // a background tab leaves the current view alone.
    if (id === currentSessionId) {
      if (next.length === 0) {
        setCurrentSession(null); // last tab closed → welcome home
      } else {
        // Jump to the left-neighbor tab; if we closed the first one (no left
        // neighbor), fall to the new first tab.
        setCurrentSession(next[Math.max(0, idx - 1)]);
      }
      setActiveTab('chat');
    }
  }, [openSessionTabs, currentSessionId, setCurrentSession]);

  // Fill the composer from a welcome-screen capability card (not auto-sent).
  const handlePickPrompt = useCallback((text: string) => {
    setInputText(text);
  }, []);

  // ---- Load persisted sessions on mount ----
  // The app opens to the Meyatu welcome screen (no active
  // conversation): we deliberately do NOT auto-select a past session. A session
  // is created lazily when the user sends their first message, or explicitly via
  // "New session" / picking a recent.
  useEffect(() => {
    loadSessions();
  }, []); // eslint-disable-line react-hooks/exhaustive-deps


  // ---- Send handler (input-owning wrapper around agent.send) ----
  // agent.send() handles the interrupt case internally: if the agent is
  // busy, it adds the user message to the store (visible immediately),
  // soft-cancels the current turn, and re-invokes send() from onDone.
  const handleSend = useCallback((overrideText?: string) => {
    const text = (overrideText ?? inputText).trim();
    if (text.length === 0 && imageAttachments.length === 0) return;

    setInputText('');
    setShowWelcome(false);
    agent.send(text, imageAttachments);
    setImageAttachments([]);
  }, [inputText, imageAttachments, agent]);

  // ---- Session selection (restores workspace if session has one) ----
  const handleSelectSession = useCallback(
    (id: string) => {
      setCurrentSession(id);
      setShowWelcome(false);
      dismissError();
      setOpenSessionTabs((prev) => prev.includes(id) ? prev : [...prev, id]);
      // Restore the session's workspace directory if it has one.
      const session = useSessionStore.getState().sessions.find((s) => s.id === id);
      if (session?.workspacePath) {
        typedInvoke<string>('tool_set_workspace', { path: session.workspacePath }).then((canonical) => {
          setWorkspacePath(canonical as string);
        }).catch(() => {
          // Directory may no longer exist — still update UI to show the stored path
          setWorkspacePath(session.workspacePath || '.');
        });
        // eslint-disable-next-line (no else — leave cwd as-is if session has no workspace)
      }
    },
    [setCurrentSession],
  );

  // ---- New session — opens dialog to pick workspace ----
  const handleNewSession = useCallback(() => {
    setNewSessionDialogOpen(true);
    setShowWelcome(false);
    dismissError();
  }, []);


  const handleNewSessionConfirm = useCallback(async (workspacePath: string | null) => {
    setNewSessionDialogOpen(false);
    setShowWelcome(false);
    // Always set the workspace FIRST so the session is created with the
    // correct cwd. createSession stores the path; setCurrentSession
    // triggers a render that picks up the canonical path from state.
    if (workspacePath) {
      await selectWorkspace(workspacePath);
    }
    const newId = createSession(undefined, undefined, workspacePath ?? undefined);
    setCurrentSession(newId);
    dismissError();
  }, [createSession, selectWorkspace, setCurrentSession]);

  // ---- Delete session ----
  const handleDeleteSession = useCallback(
    (id: string) => {
      deleteSession(id);
    },
    [deleteSession],
  );


  const handleCopy = useCallback((content: string) => {
    navigator.clipboard.writeText(content).catch(() => {});
  }, []);

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      const mod = e.ctrlKey || e.metaKey;
      if (mod && e.key === 'n') {
        e.preventDefault();
        handleNewSession();
      }
      if (mod && e.key === 'k') {
        e.preventDefault();
        setCommandPaletteOpen((p) => !p);
      }
      if (mod && e.key === 'w') {
        e.preventDefault();
        if (activeTab !== 'chat') handleCloseTab(activeTab);
      }
      if (e.key === 'Escape') {
        handleCancel();
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    return () => window.removeEventListener('keydown', handleKeyDown);
  }, [handleNewSession, handleCancel, activeTab, handleCloseTab]);

  // ---- Build the full message list (store messages + ephemeral streaming) ----
  const displayMessages: Message[] = useMemo(() => {
    // Only surface the ephemeral assistant bubble once it actually has content
    // or tool calls; until then the bottom thinking indicator stands in for it,
    // so the conversation never shows an empty assistant block with a lone cursor.
    const showEphemeral =
      streamingMsgId !== null &&
      streamingSessionId === currentSessionId &&
      (streamingContent.length > 0 || streamingToolCalls.length > 0);
    if (!showEphemeral) {
      return currentMessages;
    }
    return [
      ...currentMessages,
      ephemeralMessage(`${streamingMsgId}__streaming`, streamingContent, streamingToolCalls),
      // Use a suffixed ID to avoid React key collision with the persisted message
    ];
  }, [currentMessages, streamingMsgId, streamingSessionId, currentSessionId, streamingContent, streamingToolCalls]);

  // ---- Image attachments ----
  const handleAddImages = useCallback((images: ImageAttachment[]) => {
    setImageAttachments((prev) => [...prev, ...images]);
  }, []);
  const handleRemoveImage = useCallback((id: string) => {
    setImageAttachments((prev) => prev.filter((img) => img.id !== id));
  }, []);

  // ---- Auto-scroll ----
  const messagesContainerRef = useRef<HTMLDivElement>(null);
  const [showScrollToBottom, setShowScrollToBottom] = useState(false);

  useEffect(() => {
    const container = messagesContainerRef.current;
    if (container !== null) {
      container.scrollTop = container.scrollHeight;
    }
  }, [displayMessages]);

  const checkScrollPosition = useCallback(() => {
    const container = messagesContainerRef.current;
    if (container === null) return;
    const threshold = 100; // px from bottom
    const isNearBottom = container.scrollHeight - container.scrollTop - container.clientHeight < threshold;
    setShowScrollToBottom(!isNearBottom);
  }, []);

  const scrollToBottom = useCallback(() => {
    const container = messagesContainerRef.current;
    if (container === null) return;
    container.scrollTo({ top: container.scrollHeight, behavior: 'smooth' });
  }, []);

  // ---- Render ----
  const activeSession = getCurrentSession();

  return (
    <div className={styles.chatWindow}>
      <TitleBar
        onToggleSidebar={() => setSidebarOpen((v) => !v)}
        onSearch={() => { setSidebarOpen(true); setSearchFocusKey((k) => k + 1); }}
        onOpenSettings={() => navigate('settings')}
        onToggleFiles={() => { setSidebarOpen(true); setSidebarPane("files"); }}
        onNewSession={handleNewSession}
        onCommandPalette={() => setCommandPaletteOpen(true)}
      />

      <div className={styles.appBody}>
        {sidebarOpen && (
        <aside className={styles.sidebar} role="navigation" aria-label="Chat sessions">
          {sidebarPane === "sessions" ? (
            <SessionList sessions={sessions} currentSessionId={currentSessionId}
              providers={providers} onSelect={handleSelectSession} onNew={handleNewSession}
              onDelete={handleDeleteSession} onRename={renameSession}
              onCustomize={() => navigate('settings')} focusSearchKey={searchFocusKey} sidebarMode={sidebarPane} onToggleSidebarMode={setSidebarPane} />
          ) : (
            <>
              {/* Compact toggle bar when in files mode */}
              <div className={styles.sidebarFileToggle}>
                <button type="button"
                  className={styles.sidebarFileToggleBtn}
                  onClick={() => setSidebarPane("sessions")}
                >← Sessions</button>
                <span className={styles.sidebarFileToggleLabel}>Files</span>
              </div>
              <div className={styles.sidebarFileTreeWrapper}>
                <FileTree workspacePath={workspacePath} onSelectFile={(_path: string) => {}}
                  onOpenFile={handleOpenFile} />
              </div>
            </>
          )}

          <div className={styles.sidebarFooter}>
            {gatewayOpen && (
              <>
                <div className={styles.gatewayOverlay} onClick={() => setGatewayOpen(false)} />
                <div className={styles.gatewayMenu} role="menu">
                  <div className={styles.gatewayMenuTitle}>Switch provider</div>
                  {providers.map((p) => (
                    <button
                      key={p.id}
                      type="button"
                      role="menuitem"
                      className={`${styles.gatewayMenuItem}${p.id === activeProviderId ? ` ${styles['gatewayMenuItem--active']}` : ''}`}
                      onClick={() => { setActiveProvider(p.id); setGatewayOpen(false); }}
                    >
                      <span className={styles.gatewayMenuDot} />
                      {p.name}
                    </button>
                  ))}
                  <button
                    type="button"
                    role="menuitem"
                    className={`${styles.gatewayMenuItem} ${styles['gatewayMenuItem--manage']}`}
                    onClick={() => { setGatewayOpen(false); navigate('settings'); }}
                  >
                    Manage providers…
                  </button>
                </div>
              </>
            )}
            <button
              type="button"
              className={styles.gateway}
              onClick={() => setGatewayOpen((v) => !v)}
              aria-haspopup="menu"
              aria-expanded={gatewayOpen}
            >
              <span className={styles.gatewayIcon} aria-hidden="true">
                <BrandIcon size={16} />
              </span>
              <span className={styles.gatewayLabel}>
                {providers.find((p) => p.id === activeProviderId)?.name ?? 'Gateway'}
              </span>
              <svg className={styles.gatewayChevron} viewBox="0 0 12 12" width="11" height="11" fill="none" aria-hidden="true">
                <path d="M2 4.5L6 8l4-3.5" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round" />
              </svg>
            </button>
          </div>
        </aside>
        )}

        <main className={styles.chatArea} aria-label="Workspace">
          <div className={styles.sessionHeader}>
            {!isEmpty && activeSession ? (
              editingTitle !== null ? (
                <input
                  ref={titleInputRef}
                  className={styles.sessionHeaderInput}
                  value={editingTitle}
                  onChange={(e) => setEditingTitle(e.target.value)}
                  onBlur={() => {
                    if (editingTitle.trim()) {
                      renameSession(activeSession.id, editingTitle.trim());
                    }
                    setEditingTitle(null);
                  }}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') {
                      if (editingTitle.trim()) {
                        renameSession(activeSession.id, editingTitle.trim());
                      }
                      setEditingTitle(null);
                    }
                    if (e.key === 'Escape') {
                      setEditingTitle(null);
                    }
                  }}
                />
              ) : (
                <button type="button"
                  className={styles.sessionHeaderCrumb}
                  onClick={() => setSessionDropdownOpen((v) => !v)}
                  onDoubleClick={() => {
                    setEditingTitle(activeSession.title);
                    requestAnimationFrame(() => titleInputRef.current?.select());
                  }}
                  title="Double-click to rename"
                >
                  <svg viewBox="0 0 16 16" width="13" height="13" fill="none" aria-hidden="true">
                    <rect x="2" y="3" width="12" height="8" rx="1" stroke="currentColor" strokeWidth="1.2" />
                    <path d="M5.5 13.5h5" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" />
                  </svg>
                  <span className={`${styles.sessionHeaderName} truncate`}>{activeSession.title}</span>
                  <svg className={styles.sessionHeaderCaret} viewBox="0 0 12 12" width="10" height="10" fill="none" aria-hidden="true">
                    <path d="M2 4.5L6 8l4-3.5" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round" />
                  </svg>
                </button>
              )
            ) : (
              <span className={styles.sessionHeaderCrumb} />
            )}
            {sessionDropdownOpen && (
              <div className={styles.sessionHeaderDropdown}>
                <div className={styles.sessionHeaderDropdownHeading}>Recent sessions</div>
                {sessions.slice(0, 5).map((s) => (
                  <button
                    key={s.id}
                    type="button"
                    className={`${styles.sessionHeaderDropdownItem}${s.id === currentSessionId ? ` ${styles['sessionHeaderDropdownItem--active']}` : ''}`}
                    onClick={() => { handleSelectSession(s.id); setSessionDropdownOpen(false); }}
                  >
                    <span className={styles.sessionHeaderDropdownName}>{s.title}</span>
                    <span className={styles.sessionHeaderDropdownDate}>
                      {new Date(s.updatedAt).toLocaleDateString([], { month: 'short', day: 'numeric' })}
                    </span>
                  </button>
                ))}
                {sessions.length === 0 && <div className={styles.sessionHeaderDropdownEmpty}>No recent sessions</div>}
              </div>
            )}
            <div className={styles.sessionHeaderActions}>
              <button type="button"
                className={`${styles.sessionHeaderPanel}${detailsOpen ? ` ${styles['sessionHeaderPanel--active']}` : ''}`}
                onClick={() => setDetailsOpen((v) => !v)}
                aria-label="Toggle details panel" aria-pressed={detailsOpen} title="Details">
                <svg viewBox="0 0 16 16" width="15" height="15" fill="none" aria-hidden="true">
                  <rect x="1.5" y="2.5" width="13" height="11" rx="2" stroke="currentColor" strokeWidth="1.3" />
                  <path d="M10 2.5v11" stroke="currentColor" strokeWidth="1.3" />
                </svg>
              </button>
              <button type="button"
                className={`${styles.sessionHeaderPanel}${terminalOpen ? ` ${styles['sessionHeaderPanel--active']}` : ''}`}
                onClick={() => {
                  terminalSessionIdRef.current = `term-${Date.now()}`;
                  terminalInitialCommandRef.current = '';
                  setTerminalOpen((v) => !v);
                }}
                aria-label="Toggle terminal" aria-pressed={terminalOpen} title="Terminal">
                <svg viewBox="0 0 16 16" width="15" height="15" fill="none" aria-hidden="true">
                  <rect x="1.5" y="2.5" width="13" height="11" rx="2" stroke="currentColor" strokeWidth="1.3" />
                  <path d="M4 7l3 2.5L4 12" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" strokeLinejoin="round" />
                  <path d="M8.5 12h4" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" />
                </svg>
              </button>
            </div>
          </div>
          {(openFiles.length > 0 || openSessionTabs.length > 0) && (<div className={styles.tabBar} role="tablist" aria-label="Tabs">
              {openSessionTabs.map((sid) =>
                <div key={sid}
                  className={`${styles.tabBarTab}${activeTab === 'chat' && currentSessionId === sid ? ` ${styles['tabBarTab--active']}` : ''}`}
                  role="tab" aria-selected={activeTab === 'chat' && currentSessionId === sid}
                  onClick={() => { handleSelectSession(sid); setActiveTab('chat'); }}>
                  <span className={styles.tabBarTabLabel}>
                    {(sessions.find((s) => s.id === sid)?.title) ?? 'Session…'}
                  </span>
                  <button type="button" className={styles.tabBarClose} aria-label="Close session tab"
                    onClick={(e) => { e.stopPropagation(); handleCloseSessionTab(sid); }}>x</button>
                </div>
              )}
              {openFiles.map((file) => (
                <div key={file.path} className={`${styles.tabBarTab}${activeTab === file.path ? ` ${styles['tabBarTab--active']}` : ''}`}
                  role="tab" aria-selected={activeTab === file.path} onClick={() => setActiveTab(file.path)}>
                  <span className={styles.tabBarTabLabel}>{file.path.split('/').pop() ?? file.path}</span>
                  <button type="button" className={styles.tabBarClose} aria-label={`Close ${file.path.split('/').pop()}`}
                    onClick={(e) => { e.stopPropagation(); handleCloseTab(file.path); }}>x</button>
                </div>
              ))}
          </div>)}

          {activeTab === 'chat' && (<>
                <div ref={messagesContainerRef} className={styles.messagesContainer} role="log" aria-live="polite" onScroll={checkScrollPosition} style={{ flex: 1 }}>
                {isEmpty || showWelcome ? (
                  <WelcomeScreen onPickPrompt={handlePickPrompt} />
                ) : (
                  <>
                    {error !== null && (
                      <div className={styles.errorBanner} role="alert">
                        <span>
                          {errorLabel(error.code, t)}：{error.message}
                          {error.retryable ? ' ' + t('chat.error.retryHint') : ''}
                        </span>
                        <div className={styles.errorBannerActions}>
                          {error.code === 'SESSION_LIMIT' && (
                            <button
                              type="button"
                              className={styles.errorBannerAction}
                              onClick={handleNewSession}
                            >
                              {t('chat.newSessionLink')}
                            </button>
                          )}
                          <button
                            type="button"
                            className={styles.errorBannerDismiss}
                            onClick={dismissError}
                            aria-label="Dismiss error"
                          >
                            x
                          </button>
                        </div>
                      </div>
                    )}

                    <div className={styles.messagesList}>
                      {displayMessages.map((msg, index) => {
                        const isLast = index === displayMessages.length - 1;
                        const isStreamingMsg =
                          isLast && streamingMsgId !== null && msg.id === `${streamingMsgId}__streaming`;
                        const isLastAssistant = isLast && msg.role === 'assistant' && !isStreamingMsg;

                        return (
                          <ErrorBoundary
                            key={msg.id}
                          >
                            <MessageBubble
                              message={msg}
                              isStreaming={isStreamingMsg}
                              showTimestamp={showTimestamps}
                              onCopy={handleCopy}
                              onRetry={isLastAssistant ? handleRetry : undefined}
                              onRevertCheckpoint={msg.checkpointId ? () => handleRevertCheckpoint(msg.checkpointId!) : undefined}
                              tokenUsage={isLastAssistant ? getCurrentSession()?.tokens : undefined}
                            />
                          </ErrorBoundary>
                        );
                      })}
                    </div>

                    {/* Show the live indicator whenever the agent is working —
                        not just when streaming content is empty — so the user
                        always sees the animated sparkle + elapsed time at the
                        bottom while the model is thinking or running tools. */}
                    {streamingForCurrent && (
                      <div className={`${styles.messagesList} ${styles['messagesList--indicator']}`}>
                        <ThinkingIndicator
                          elapsedSec={elapsedSec}
                          tokens={thinkingTokens}
                          status={thinkingStatus}
                          activeTools={activeTools}
                        />
                      </div>
                    )}
                    {parallelRun && agentBusy && streamingSessionId === currentSessionId && (
                      <div className={`${styles.messagesList} ${styles['messagesList--parallel']}`}>
                        <ParallelAgentsCard agents={Object.values(parallelRun.agents)} />
                      </div>
                    )}
                  </>
                )}
              </div>

              {showScrollToBottom && (
                <button
                  type="button"
                  className={styles.scrollToBottomBtn}
                  onClick={scrollToBottom}
                  aria-label="Scroll to bottom"
                  title="Scroll to bottom"
                >
                  <svg viewBox="0 0 16 16" width="14" height="14" fill="none" aria-hidden="true">
                    <path d="M8 3v10M4.5 9l3.5 3.5L11.5 9" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
                  </svg>
                </button>
              )}

              <ChatInput
                value={inputText}
                onChange={setInputText}
                onSend={handleSend}
                onCancel={handleCancel}
                isStreaming={agentBusy}
                pendingInterrupt={pendingInterrupt?.text ?? null}
                models={activeModels}
                selectedModel={effectiveModel}
                onModelChange={setSelectedModel}
                disabled={false}
                toolCount={REGISTRY_DEFS.length}
                contextLabel={
                  workspacePath && workspacePath !== '.'
                    ? (workspacePath.split(/[\\/]/).filter(Boolean).pop() ?? workspacePath)
                    : ''
                }
                onContextClick={() => { setSidebarOpen(true); setSidebarPane("files"); }}
                onAttachFile={async () => {
                  try {
                    const path = await typedInvoke<string | null>('tool_pick_file');
                    if (!path) return;
                    const content = await typedInvoke<string>('tool_read_attachment', { path });
                    const name = path.split(/[\\/]/).pop() ?? path;
                    setInputText((prev) => `${prev}\n\n[Attached file: ${name}]\n\`\`\`\n${content}\n\`\`\`\n`);
                  } catch (e) {
                    agent.reportError('ATTACH_FAILED', `${t('chat.error.attachFailed')}：${normalizeError(e).message}`);
                  }
                }}
                imageAttachments={imageAttachments}
                onAddImages={handleAddImages}
                onRemoveImage={handleRemoveImage}
                permissionMode={permissionMode}
                onSelectPermissionMode={setPermissionMode}
                contextTokens={activeSession?.tokens.totalTokens ?? 0}
                contextType={contextType}
                onSelectContextType={(type) => { setContextType(type); }}
                inSession={!isEmpty}
                workspacePath={workspacePath}
                workspacePaths={workspacePaths}
                onRemoveWorkspacePath={removePath}
                providerStatus={providersConfigured ? undefined : 'No API key — set one in Settings'}
                isOffline={!online}
                onOpenTerminalWithCommand={(command) => {
                  terminalSessionIdRef.current = `term-${Date.now()}`;
                  terminalInitialCommandRef.current = command;
                  setTerminalOpen(true);
                }}
              />
            </>
          )}

          {/* Code editor view */}
          {activeTab !== 'chat' && (() => {
            const file = openFiles.find((f) => f.path === activeTab);
            if (!file) return <div className={codeEditorStyles.codeEditor}><div className={codeEditorStyles.codeEditorError}>File not found</div></div>;
            return (
              <CodeEditor
                filePath={file.path}
                content={file.content}
              />
            );
          })()}

          {/* Terminal panel — wrapped in ErrorBoundary to prevent white-screen crash */}
          {terminalOpen && (
            <ErrorBoundary
              fallback={(error, _retry) => (
                <div className={terminalStyles.terminalPanel}>
                  <div className={terminalStyles.terminalPanelHeader}>
                    <span className={terminalStyles.terminalPanelTitle}>Terminal</span>
                    <button
                      type="button"
                      className={terminalStyles.terminalPanelBtn}
                      onClick={() => setTerminalOpen(false)}
                      title="Close terminal"
                    >
                      ✕
                    </button>
                  </div>
                  <div className="terminal-panel__body" style={{ flex: 1, display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', gap: 4, padding: '0 12px', fontSize: 12, color: 'var(--color-text-muted)' }}>
                    <span>Terminal encountered an error:</span>
                    <code style={{ color: 'var(--color-error)', fontSize: 11, textAlign: 'center', wordBreak: 'break-all', maxWidth: '80%' }}>{error.message}</code>
                    <div style={{ display: 'flex', gap: 8, marginTop: 8 }}>
                      <button type="button" className={terminalStyles.terminalPanelBtn} style={{ padding: '4px 8px', fontSize: 12 }} onClick={() => { setTerminalOpen(false); setTimeout(() => setTerminalOpen(true), 0); }}>Retry</button>
                      <button type="button" className={terminalStyles.terminalPanelBtn} style={{ padding: '4px 8px', fontSize: 12 }} onClick={() => setTerminalOpen(false)}>Close</button>
                    </div>
                  </div>
                </div>
              )}
            >
              <Terminal
                workspacePath={workspacePath}
                onClose={() => setTerminalOpen(false)}
                sessionId={terminalSessionIdRef.current}
                initialCommand={terminalInitialCommandRef.current}
                onStarted={() => {
                  terminalInitialCommandRef.current = '';
                }}
              />
            </ErrorBoundary>
          )}
        </main>

        {/* Right detail panel — session info */}
        {detailsOpen && (
          <aside className={rightPanelStyles.rightPanel} aria-label="Session details">
            <div className={rightPanelStyles.rightPanelHeader}>
              <span className={rightPanelStyles.rightPanelTitle}>Details</span>
              <button className={rightPanelStyles.rightPanelClose} onClick={() => setDetailsOpen(false)} title="Close panel" aria-label="Close details">×</button>
            </div>
            <div className={`${rightPanelStyles.rightPanelBody} ${styles.detailsPanel}`}>
              {parallelRun && (
                <div className={styles.detailsPanelSection}>
                  <div className={styles.detailsPanelSectionTitle}>Parallel Agents</div>
                  <AgentsPanel run={parallelRun} />
                </div>
              )}
              {planSteps.length > 0 && (
                <div className={styles.detailsPanelPlan}>
                  <div className={styles.detailsPanelSectionTitle}>Task Plan</div>
                  {planSteps.map((step) => (
                    <div key={step.id} className={`${styles.detailsPanelStep} ${styles[`detailsPanelStep--${step.status}`] ?? ''}`}>
                      <span className={styles.detailsPanelStepStatus} aria-label={step.status}>
                        {step.status === 'completed' ? '✓' : step.status === 'failed' ? '✗' : step.status === 'in_progress' ? '●' : '○'}
                      </span>
                      <span className={styles.detailsPanelStepDesc}>{step.description}</span>
                    </div>
                  ))}
                </div>
              )}
              {activeSession ? (
                <>
                  <dl className={styles.detailsPanelList}>
                    <div className={styles.detailsPanelRow}><dt>Session</dt><dd>{activeSession.title}</dd></div>
                    <div className={styles.detailsPanelRow}><dt>Provider</dt><dd>{providers.find((p) => p.id === activeProviderId)?.name ?? '—'}</dd></div>
                    <div className={styles.detailsPanelRow}><dt>Model</dt><dd>{activeSession.model ?? effectiveModel ?? '—'}</dd></div>
                    <div className={styles.detailsPanelRow}><dt>Messages</dt><dd>{activeSession.messages.filter((m) => m.role === 'user' || m.role === 'assistant').length}</dd></div>
                    <div className={styles.detailsPanelRow}><dt>Workspace</dt><dd title={workspacePath}>{(function() { try { return (workspacePath && workspacePath !== '.') ? (String(workspacePath).split(/[\\/]/).filter(Boolean).pop() || 'root') : '—'; } catch { return '—'; } })()}</dd></div>
                  </dl>
                  <TokenDashboard />
                  {thinkingTokens > 0 && (
                    <div className={styles.detailsPanelSection}>
                      <div className={styles.detailsPanelSectionTitle}>Thinking</div>
                      <div className={styles.detailsPanelRow}><dt>Tokens</dt><dd>{(thinkingTokens / 1000).toFixed(1)}k</dd></div>
                    </div>
                  )}
                  {planSteps.length === 0 && activeSession.messages.filter((m) => m.role === 'tool' || m.role === 'assistant').length > 0 && (
                    <div className={styles.detailsPanelSection}>
                      <div className={styles.detailsPanelSectionTitle}>Session Stats</div>
                      <div className={styles.detailsPanelRow}><dt>Assistant</dt><dd>{activeSession.messages.filter((m) => m.role === 'assistant').length}</dd></div>
                      <div className={styles.detailsPanelRow}><dt>Tool calls</dt><dd>{activeSession.messages.filter((m) => m.role === 'tool').length}</dd></div>
                    </div>
                  )}
                </>
              ) : (
                <p className="details-panel__empty">No active session. Start or select a conversation to see details here.</p>
              )}
            </div>
          </aside>
        )}
      </div>

      <NewSessionDialog
        open={newSessionDialogOpen}
        onClose={() => setNewSessionDialogOpen(false)}
        onConfirm={handleNewSessionConfirm}
      />

      <CommandPalette
            open={commandPaletteOpen}
            commands={[
              { id: 'settings', label: 'Open Settings', action: () => { navigate('settings'); setCommandPaletteOpen(false); } },
              { id: 'new-session', label: 'New Session', shortcut: '⌘N', action: () => { handleNewSession(); setCommandPaletteOpen(false); } },
              { id: 'focus-search', label: 'Search Sessions', action: () => { setSearchFocusKey((k) => k + 1); setCommandPaletteOpen(false); } },
            ]}
            onClose={() => setCommandPaletteOpen(false)}
          />

      {pendingPermission !== null && (
        <PermissionDialog
          toolCall={pendingPermission}
          onDecide={resolvePermission}
        />
      )}

      <FileSyncNotifications />
      <Toaster />
    </div>
  );
}
