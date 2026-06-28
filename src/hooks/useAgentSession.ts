import { useState, useRef, useCallback, useEffect } from 'react';
import { typedInvoke } from '../utils/ipc';
import type { Message, ToolCall, ToolResult, PlanStep } from '../types/message';
import type { TokenUsage } from '../types/session';
import type { AgentContext, AgentCallbacks, ToolProgressInfo } from '../agent/loop';
import { AgentLoop } from '../agent/loop';
import { TauriToolExecutor } from '../agent/toolExecutor';
import { BASE_SYSTEM_PROMPT } from '../agent/systemPrompt';
import { buildUserContent, buildPartialMessages, buildDoneMessages } from '../agent/sessionMessages';
import { useChatStream } from './useChatStream';
import { usePermissionGate, type PermissionMode } from './usePermissionGate';
import { useOnlineStatus } from './useOnlineStatus';

/** Strip <think>...</think> blocks from model output so reasoning stays invisible.
 *  Handles fully-closed blocks and unclosed opening tags (mid-stream). */
function stripThinkBlocks(text: string): string {
  let result = text.replace(/<think>[\s\S]*?<\/think>/gi, '');
  const openIdx = result.lastIndexOf('<think>');
  if (openIdx !== -1) {
    result = result.slice(0, openIdx);
  }
  return result;
}
import { useSessionStore } from '../stores/sessionStore';
import { useProviderStore } from '../stores/providerStore';
import { reduceParallel, type ParallelRunState } from '../agent/parallelEvents';
import { estimateCost } from '../utils/pricing';
import type { ImageAttachment } from '../components/ChatInput';
import type { PermissionDecision } from '../agent/tools';

/** Inputs the agent-run orchestration needs from the surrounding UI shell. */
export interface UseAgentSessionOptions {
  /** Active workspace directory (used for session creation + checkpoint label). */
  workspacePath: string;
  /** Workspace codebase index appended to the system prompt, if available. */
  workspaceIndex: string;
  /** The effective model id for the active provider/session. */
  effectiveModel: string | undefined;
  /** Called when a parallel dispatch begins (e.g. to open the Agents panel). */
  onParallelStart?: () => void;
}

export interface UseAgentSession {
  send: (text: string, images: ImageAttachment[]) => void;
  cancel: () => void;
  /** Soft cancel: let current iteration finish, then stop. Used for 追问 (interrupt). */
  softCancel: () => void;
  retry: () => void;
  dismissError: () => void;
  reportError: (code: string, message: string) => void;
  streamingContent: string;
  streamingMsgId: string | null;
  /** Session the in-flight stream belongs to, so the UI only renders the live
   *  bubble/indicator in that session's view (not whatever tab is active). */
  streamingSessionId: string | null;
  streamingToolCalls: ToolCall[];
  agentBusy: boolean;
  error: { code: string; message: string; retryable?: boolean } | null;
  thinkingStatus: string;
  thinkingTokens: number;
  elapsedSec: number;
  activeTools: Map<string, { toolName: string; status: string }>;
  planSteps: PlanStep[];
  parallelRun: ParallelRunState | null;
  /** Follow-up text queued while the agent is busy (shown above input). */
  pendingInterrupt: { text: string } | null;
  pendingPermission: ToolCall | null;
  resolvePermission: (decision: PermissionDecision) => void;
  permissionMode: PermissionMode;
  setPermissionMode: (mode: PermissionMode) => void;
}

/**
 * Owns the agent-run orchestration extracted from `ChatWindow`: the send/stream/
 * persist loop, streaming state, the agent callbacks, permission gating, the
 * stream watchdog hook, and cancel/retry. The UI shell (tabs, sidebar, input
 * draft, file tree) stays in `ChatWindow`, which consumes this hook's API.
 */
export function useAgentSession(opts: UseAgentSessionOptions): UseAgentSession {
  // Always-fresh view of the caller-provided inputs (read inside `send` without
  // forcing `send` to be re-created when they change).
  const optsRef = useRef(opts);
  optsRef.current = opts;

  // ---- Stores ----
  const { createSession, addMessage, addTokens, getCurrentSession, renameSession } = useSessionStore();
  const { providers, activeProviderId } = useProviderStore();
  const providersConfigured = activeProviderId !== null;

  // ---- Connectivity (offline send-gate) ----
  const online = useOnlineStatus();
  const onlineRef = useRef(online);
  useEffect(() => { onlineRef.current = online; }, [online]);

  // ---- Streaming hook (+ watchdog) ----
  const { streamChat, cancel: cancelStream } = useChatStream();

  const toolExecutorRef = useRef<TauriToolExecutor>(new TauriToolExecutor());
  const agentLoopRef = useRef<AgentLoop>(new AgentLoop(streamChat, cancelStream, toolExecutorRef.current, () => streamChat));
  const consecutiveErrorsRef = useRef(0);

  // ---- Streaming state ----
  const [streamingContent, setStreamingContent] = useState('');
  const [streamingToolCalls, setStreamingToolCalls] = useState<ToolCall[]>([]);
  const [streamingMsgId, setStreamingMsgId] = useState<string | null>(null);
  // The session the in-flight stream belongs to (the session active at send
  // time). State drives rendering; the ref lets finalizePartial/onDone persist
  // to the originating session even after the user switches tabs mid-stream.
  const [streamingSessionId, setStreamingSessionId] = useState<string | null>(null);
  const streamingSessionIdRef = useRef<string | null>(null);
  const streamingToolResultsRef = useRef<ToolResult[]>([]);
  // Refs mirror the streaming content/tool-calls so onDone can read the final
  // value and persist the message exactly once (StrictMode double-fire safety).
  const streamingContentRef = useRef('');
  const streamingToolCallsRef = useRef<ToolCall[]>([]);
  // rAF handle for throttling setStreamingContent to once per animation frame.
  // Avoids React re-render on every SSE token (~10-50 ms), which causes janky
  // "jumping" text during streaming.
  const streamingRafRef = useRef<number | null>(null);

  // Helper: flush any rAF-pending content to state, then cancel the rAF.
  const flushStreamingRaf = useCallback(() => {
    if (streamingRafRef.current !== null) {
      cancelAnimationFrame(streamingRafRef.current);
      streamingRafRef.current = null;
    }
    // One final setState so the UI catches up with the ref.
    setStreamingContent(stripThinkBlocks(streamingContentRef.current));
  }, []);

  // ---- Permission gate ----
  const {
    pendingPermission,
    permissionMode,
    setPermissionMode,
    requestPermission,
    resolvePermission,
    denyPending,
  } = usePermissionGate();

  // ---- Agent-run state ----
  const [agentBusy, setAgentBusy] = useState(false);
  const agentBusyRef = useRef(false);
  const [error, setError] = useState<{ code: string; message: string; retryable?: boolean } | null>(null);
  const [planSteps, setPlanSteps] = useState<PlanStep[]>([]);

  // ---- Live "thinking" indicator ----
  const [elapsedSec, setElapsedSec] = useState(0);
  const [thinkingTokens, setThinkingTokens] = useState(0);
  const [thinkingStatus, setThinkingStatus] = useState('');
  const [activeTools, setActiveTools] = useState<Map<string, { toolName: string; status: string }>>(new Map());
  // Live parallel-agent run state. The ref mirrors state so onDone can read the
  // final snapshot without a setState race.
  const [parallelRun, setParallelRun] = useState<ParallelRunState | null>(null);
  const parallelRunRef = useRef<ParallelRunState | null>(null);
  const turnStartRef = useRef(0);

  // Tick the elapsed-time counter once per second for the whole turn.
  useEffect(() => {
    if (streamingMsgId === null) return;
    turnStartRef.current = Date.now();
    setElapsedSec(0);
    const id = setInterval(() => {
      setElapsedSec(Math.floor((Date.now() - turnStartRef.current) / 1000));
    }, 1000);
    return () => clearInterval(id);
  }, [streamingMsgId]);

  // Load MCP server tools once on mount (best-effort).
  useEffect(() => {
    void toolExecutorRef.current.loadMcpTools();
  }, []);

  // Cancel any in-flight run on unmount.
  useEffect(() => {
    return () => {
      agentLoopRef.current.cancel();
    };
  }, []);

  // ---- Save partial streaming content as a message ----
  const finalizePartial = useCallback(() => {
    if (streamingMsgId === null) {
      return;
    }
    // Persist to the session the stream belongs to — NOT getCurrentSession(),
    // which may have changed if the user switched tabs mid-stream.
    const sessionId = streamingSessionIdRef.current;
    if (sessionId === null) {
      return;
    }
    const msgs = buildPartialMessages(
      streamingMsgId,
      stripThinkBlocks(streamingContentRef.current),
      streamingToolCallsRef.current,
      streamingToolResultsRef.current,
    );
    for (const m of msgs) addMessage(sessionId, m);
  }, [streamingMsgId, addMessage]);

  // ---- Send ----
  /** Follow-up queued when user interrupts a running turn ("追问"). */
  const followUpRef = useRef<{ text: string; images: ImageAttachment[] } | null>(null);
  /** Pending interrupt chip visible above the input. */
  const [pendingInterrupt, setPendingInterrupt] = useState<{ text: string } | null>(null);
  /** Always points to the latest send so onDone can dispatch follow-ups. */
  const sendRef = useRef<(text: string, images: ImageAttachment[]) => void>(() => {});

  const send = useCallback((text: string, images: ImageAttachment[]) => {
    if (text.length === 0 && images.length === 0) {
      return;
    }

    // Interrupt: show chip, soft-cancel. onDone will add the user
    // message to the store and re-invoke sendRef.current().
    if (agentBusyRef.current) {
      followUpRef.current = { text, images };
      setPendingInterrupt({ text });
      agentLoopRef.current.softCancel();
      return;
    }

    if (!providersConfigured) {
      setError({
        code: 'NO_PROVIDER',
        message: 'No provider configured. Go to Settings and add an API key for a provider.',
      });
      return;
    }

    if (!onlineRef.current) {
      setError({ code: 'OFFLINE', message: '离线 — 无法发送，请检查网络' });
      return;
    }

    setError(null);
    consecutiveErrorsRef.current = 0;

    // Preserve any partial streaming content before starting a new turn.
    finalizePartial();

    // Lazily create a session on the first message.
    let session = getCurrentSession();
    if (session === null) {
      const newId = createSession(undefined, undefined, optsRef.current.workspacePath || undefined);
      session = useSessionStore.getState().sessions.find((s) => s.id === newId) ?? null;
      if (session === null) {
        return;
      }
    }

    const previousMessages = [...session.messages];
    const provider = providers.find((p) => p.id === activeProviderId);
    if (provider === undefined) {
      return;
    }

    if (provider.requiresApiKey && !useProviderStore.getState().configuredProviders.has(provider.id)) {
      setError({ code: 'NO_API_KEY', message: `${provider.name} 需要 API 密钥，请在设置中添加。` });
      return;
    }

    const messageContent = buildUserContent(text, images);
    const userMsg: Message = {
      id: crypto.randomUUID(),
      role: 'user',
      content: messageContent,
      timestamp: Date.now(),
    };
    addMessage(session.id, userMsg);

    if (session.title === 'New Chat') {
      const autoTitle = text.slice(0, 40) + (text.length > 40 ? '…' : '');
      renameSession(session.id, autoTitle);
    }

    setPlanSteps([]);
    setParallelRun(null);
    parallelRunRef.current = null;

    const systemPrompt = optsRef.current.workspaceIndex
      ? `${BASE_SYSTEM_PROMPT}\n\n${optsRef.current.workspaceIndex}`
      : BASE_SYSTEM_PROMPT;

    const context: AgentContext = {
      sessionId: session.id,
      messages: previousMessages,
      providerId: provider.id,
      model: optsRef.current.effectiveModel ?? provider.models[0] ?? '',
      systemPrompt,
      baseUrl: provider.baseUrl,
    };

    // Prepare streaming state.
    const assistantId = crypto.randomUUID();
    let turnCheckpointId: string | null = null;
    setStreamingMsgId(assistantId);
    setStreamingSessionId(session.id);
    streamingSessionIdRef.current = session.id;
    setStreamingContent('');
    setStreamingToolCalls([]);
    streamingToolResultsRef.current = [];
    streamingContentRef.current = '';
    streamingToolCallsRef.current = [];

    setThinkingTokens(0);
    setThinkingStatus('thinking…');

    const callbacks: AgentCallbacks = {
      onDelta: (content) => {
        streamingContentRef.current += content;
        // Throttle setState to once per animation frame (~16 ms).
        // Multiple SSE tokens arriving within one frame accumulate in the
        // ref and trigger a single React re-render, avoiding janky text.
        if (streamingRafRef.current === null) {
          streamingRafRef.current = requestAnimationFrame(() => {
            streamingRafRef.current = null;
            setStreamingContent(stripThinkBlocks(streamingContentRef.current));
          });
        }
      },
      onToolCall: (toolCall) => {
        streamingToolCallsRef.current = [...streamingToolCallsRef.current, toolCall];
        setStreamingToolCalls(streamingToolCallsRef.current);
      },
      onToolResult: (result) => {
        streamingToolResultsRef.current = [...streamingToolResultsRef.current, result];
      },
      onToolProgress: (info: ToolProgressInfo) => {
        setActiveTools(prev => {
          const next = new Map(prev);
          if (info.status === 'starting') {
            next.set(info.toolCallId, { toolName: info.toolName, status: 'running' });
          } else if (info.status === 'completed') {
            next.set(info.toolCallId, { toolName: info.toolName, status: 'done' });
          } else if (info.status === 'failed') {
            next.set(info.toolCallId, { toolName: info.toolName, status: 'error' });
          }
          return next;
        });
      },
      onTokens: (usage: TokenUsage) => {
        const cost = estimateCost(
          context.model,
          usage.promptTokens,
          usage.completionTokens,
        );
        addTokens(session.id, { ...usage, cost });
        setThinkingTokens((t) => t + usage.completionTokens);
      },
      onStateChange: (state: string, detail?: string) => {
        if (state === 'tool_call') {
          setThinkingStatus(detail ? `Running ${detail}…` : 'Running tool…');
        } else if (state === 'verifying') {
          setThinkingStatus('Running tests…');
        } else if (state === 'thinking') {
          setThinkingStatus('still thinking…');
        } else if (state === 'done' || state === 'verified' || state === 'error') {
          setThinkingStatus('');
          setActiveTools(new Map());
        }
      },
      onPermissionRequest: requestPermission,
      onError: (code, message, retryable) => {
        finalizePartial();
        flushStreamingRaf();
        consecutiveErrorsRef.current += 1;
        const extra =
          consecutiveErrorsRef.current >= 2 && providers.length > 1
            ? ' — 连续失败，建议切换 Provider'
            : '';
        setError({ code, message: message + extra, retryable });
        streamingContentRef.current = '';
        streamingToolCallsRef.current = [];
        streamingToolResultsRef.current = [];
        setStreamingMsgId(null);
        setStreamingSessionId(null);
        streamingSessionIdRef.current = null;
        setStreamingContent('');
        setStreamingToolCalls([]);
        setThinkingStatus('');
      },
      onDone: () => {
        flushStreamingRaf();
        const cleanContent = stripThinkBlocks(streamingContentRef.current);
        const doneMsgs = buildDoneMessages(
          assistantId,
          cleanContent,
          streamingToolCallsRef.current,
          streamingToolResultsRef.current,
        );
        if (turnCheckpointId && doneMsgs[0]) doneMsgs[0].checkpointId = turnCheckpointId;
        // Persist the parallel-run snapshot onto the assistant message.
        if (parallelRunRef.current && doneMsgs[0]) {
          doneMsgs[0].parallelSnapshot = Object.values(parallelRunRef.current.agents);
        }
        for (const m of doneMsgs) addMessage(session.id, m);

        streamingContentRef.current = '';
        streamingToolCallsRef.current = [];
        streamingToolResultsRef.current = [];
        setStreamingMsgId(null);
        setStreamingSessionId(null);
        streamingSessionIdRef.current = null;
        setStreamingContent('');
        setStreamingToolCalls([]);
      },
      onPlanStep: (step: PlanStep) => {
        setPlanSteps((prev) => {
          const idx = prev.findIndex((s) => s.id === step.id);
          if (idx >= 0) {
            const next = [...prev];
            next[idx] = step;
            return next;
          }
          return [...prev, step];
        });
      },
      onParallelEvent: (ev) => {
        const next = reduceParallel(parallelRunRef.current, ev);
        parallelRunRef.current = next;
        setParallelRun(next);
        if (ev.phase === 'start') optsRef.current.onParallelStart?.();
      },
    };

    agentBusyRef.current = true;
    setAgentBusy(true);
    void (async () => {
      try {
        turnCheckpointId = await typedInvoke<string>('tool_checkpoint_begin', {
          label: text,
          workspace: optsRef.current.workspacePath || '',
        });
      } catch { /* no checkpoint this turn */ }
      agentLoopRef.current
        .run(messageContent, callbacks, context)
        .catch(() => {
          // run() surfaces its own errors via onError; guard unhandled rejection.
        })
        .finally(() => {
          agentBusyRef.current = false;
          setAgentBusy(false);
          typedInvoke<void>('tool_checkpoint_end').catch(() => {});
          // Dispatch queued 追问 — sendRef.current always points to
          // the latest send, so no closure-staleness issues.
          const queued = followUpRef.current;
          if (queued) {
            followUpRef.current = null;
            setPendingInterrupt(null);
            sendRef.current(queued.text, queued.images);
          }
        });
    })();
  }, [
    providersConfigured,
    createSession,
    finalizePartial,
    renameSession,
    getCurrentSession,
    providers,
    activeProviderId,
    addMessage,
    addTokens,
    requestPermission,
    flushStreamingRaf,
  ]);
  sendRef.current = send;

  // ---- Cancel ----
  const cancel = useCallback(() => {
    denyPending();
    agentLoopRef.current.cancel();
  }, [denyPending]);

  // ---- Soft cancel (追问 — let current iteration complete) ----
  const softCancel = useCallback(() => {
    agentLoopRef.current.softCancel();
  }, []);

  // ---- Dismiss / report error ----
  const dismissError = useCallback(() => {
    setError(null);
  }, []);

  const reportError = useCallback((code: string, message: string) => {
    setError({ code, message });
  }, []);

  // ---- Retry the last user turn ----
  const retry = useCallback(() => {
    const session = getCurrentSession();
    if (!session || agentBusyRef.current) return;

    let lastUserIdx = -1;
    for (let i = session.messages.length - 1; i >= 0; i--) {
      if (session.messages[i].role === 'user') { lastUserIdx = i; break; }
    }
    if (lastUserIdx < 0) return;

    const lastUserContent = session.messages[lastUserIdx].content;
    const lastUserText = typeof lastUserContent === 'string'
      ? lastUserContent
      : lastUserContent.filter((p) => p.type === 'text').map((p) => p.text ?? '').join('');
    const newMessages = session.messages.slice(0, lastUserIdx);
    useSessionStore.setState((state) => ({
      sessions: state.sessions.map((s) =>
        s.id === session.id ? { ...s, messages: newMessages, updatedAt: Date.now() } : s,
      ),
    }));

    send(lastUserText, []);
  }, [getCurrentSession, send]);

  // Cancel any pending rAF on unmount.
  useEffect(() => () => {
    if (streamingRafRef.current !== null) {
      cancelAnimationFrame(streamingRafRef.current);
      streamingRafRef.current = null;
    }
  }, []);

  return {
    send,
    cancel,
    softCancel,
    retry,
    dismissError,
    reportError,
    streamingContent,
    streamingMsgId,
    streamingSessionId,
    streamingToolCalls,
    agentBusy,
    error,
    thinkingStatus,
    thinkingTokens,
    elapsedSec,
    activeTools,
    planSteps,
    parallelRun,
    pendingInterrupt,
    pendingPermission,
    resolvePermission,
    permissionMode,
    setPermissionMode,
  };
}
