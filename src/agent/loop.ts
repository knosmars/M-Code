import type { ChatRequest } from '../types/ipc';
import type { Message, ContentPart } from '../types/message';
import type { ToolCall } from '../types/message';
import type { ToolResult } from '../types/message';
import type { PlanStep } from '../types/message';
import { getTextContent } from '../types/message';
import type { StreamEvent } from '../types/stream';
import type { ToolExecutor, PermissionDecision } from './tools';
import type { TokenUsage } from '../types/session';
import type { ParallelAgentEvent } from './parallelEvents';
import { classifyModelTier, getFallbackModel, type ModelTier } from '../stores/providerStore';

/** Callbacks the UI layer provides to react to agent loop events.
 *
 * Every callback is optional — the UI subscribes to only the
 * events it cares about.  The agent loop guarantees that
 * `onDone` fires exactly once per `run()` call (on success
 * or after `onError` on failure).
 */
/** Real-time progress event for a single tool execution. */
export interface ToolProgressInfo {
  toolName: string;
  toolCallId: string;
  status: 'starting' | 'completed' | 'failed';
  error?: string;
}

export interface AgentCallbacks {
  /** Incremental text content from the LLM response */
  onDelta?: (content: string) => void;
  /** A tool call requested by the LLM (function calling) */
  onToolCall?: (toolCall: ToolCall) => void;
  /** The result of an executed tool call */
  onToolResult?: (result: ToolResult) => void;
  /** Permission gate: called before executing a tool with side effects.
   *  Must resolve to allow/deny/always_allow. Tools without side effects
   *  skip this callback. */
  onPermissionRequest?: (toolCall: ToolCall) => Promise<PermissionDecision>;
  /** Agent state machine transition (DEVELOPMENT_GUIDE §6.2) */
  onStateChange?: (state: string, detail?: string) => void;
  onTokens?: (tokens: TokenUsage) => void;
  /** A structured error from the backend (provider, network, etc.) */
  onError?: (code: string, message: string, retryable?: boolean) => void;
  /** Sentinel indicating the stream has completed (success or after error) */
  onDone?: () => void;
  /** Hook lifecycle event fired for before_chat, after_chat, before_tool, after_tool */
  onHookEvent?: (event: string, details: string) => void;
  /** Plan step status update during autonomous execution */
  onPlanStep?: (step: PlanStep) => void;
  /** Real-time tool execution progress (starting/completed/failed per tool) */
  onToolProgress?: (info: ToolProgressInfo) => void;
  /** Structured per-sub-agent progress during a parallel dispatch. */
  onParallelEvent?: (event: ParallelAgentEvent) => void;
}

/** Context the agent loop needs to construct a valid `ChatRequest`. */
export interface AgentContext {
  /** The current chat session ID */
  sessionId: string;
  /** Full conversation history prior to the new user message */
  messages: Message[];
  /** Active provider identifier (e.g. "openai-compatible") */
  providerId: string;
  /** Active model name (e.g. "gpt-4o") */
  model: string;
  /** Optional system prompt to prepend */
  systemPrompt?: string;
  /** Optional provider base URL override (local Ollama / custom gateway). */
  baseUrl?: string;
  /** Override the maximum tool-call iterations for this run (floor: FALLBACK_MAX_TOOL_ITERATIONS) */
  maxIterations?: number;
  /** Enable autonomous mode: plan, execute, verify, and optionally create a PR */
  autonomous?: boolean;
  /** Create a GitHub PR after successful autonomous execution */
  autoPr?: boolean;
}

/** Signature of the `streamChat` function returned by `useChatStream()`. */
type StreamChatFn = (request: ChatRequest) => AsyncGenerator<StreamEvent, void, unknown>;

/** Factory that creates a new StreamChatFn instance (for parallel agents). */
type StreamChatFactory = () => StreamChatFn;

/** Abort/cancel function returned by `useChatStream()`. */
type CancelFn = () => void;

/** A sub-task dispatched to a parallel agent. */
export interface ParallelTask {
  task: string;
  systemPrompt?: string;
  /** Display label for the sub-agent (resolved agent name, or "default"). */
  agentName?: string;
}

/** Result from a single parallel sub-task execution. */
export interface ParallelResult {
  taskId: string;
  success: boolean;
  content?: string;
  error?: string;
  iterations: number;
}

/** Maximum tool-call loop iterations to prevent infinite loops.
 * Must be per-instance to avoid shared mutable state across concurrent AgentLoops. */
const FALLBACK_MAX_TOOL_ITERATIONS = 25;

/** Tool results larger than this (in characters) are middle-truncated before
 *  being fed back to the LLM. A single huge output (e.g. `cargo test`) would
 *  otherwise blow up the context window, or — worse — get cut off so the model
 *  thinks the result was incomplete and re-runs the command forever. */
const MAX_TOOL_RESULT_CHARS = 16_000;

/** Coerce any tool-result value to a string. Some Tauri commands are typed
 *  `Promise<string>` but actually return structured objects (e.g. triggers_list,
 *  index_codebase), so the runtime value isn't always a string — JSON-stringify
 *  those so the model gets readable content instead of "[object Object]", and so
 *  the string ops below never blow up ("content.slice is not a function"). */
function toResultString(content: unknown): string {
  if (typeof content === 'string') return content;
  if (content === null || content === undefined) return '';
  try {
    return JSON.stringify(content);
  } catch {
    return String(content);
  }
}

/** Semantically truncate an oversized tool result, preserving key content:
 *  - Tail-weighted (60 % budget to end — test summaries live there)
 *  - Key-line extraction (error / fail / panic lines kept in a dedicated section)
 *  - JSON-aware (truncates large arrays while preserving structure)
 *
 *  This is a context-size guardrail, not a behavioural instruction —
 *  the markers just state what happened. */
export function truncateToolResult(content: unknown): string {
  const str = toResultString(content);
  if (str.length <= MAX_TOOL_RESULT_CHARS) return str;

  const budget = MAX_TOOL_RESULT_CHARS;

  // --- 1. JSON-structured truncation ---
  const trimmed = str.trimStart();
  if (trimmed.startsWith('{') || trimmed.startsWith('[')) {
    try {
      const parsed = JSON.parse(trimmed);
      const compact = jsonTruncate(parsed, budget);
      if (compact.length <= budget) return compact;
    } catch {
      /* not valid JSON — fall through to text truncation */
    }
  }

  // --- 2. Tail-weighted textual truncation ---
  const tailBudget = Math.floor(budget * 0.6); // 60 % — test summaries live at end
  const headBudget = budget - tailBudget;       // 40 %

  // --- 3. Key-line extraction from the discarded middle ---
  const lines = str.split('\n');
  const keyPattern = /error|fail|panic|FAIL|Error|traceback|exception/i;
  const keyLines = lines.filter((l) => keyPattern.test(l));

  // Head: first headBudget chars, snapped to the preceding line boundary
  let head = str.slice(0, Math.min(headBudget, str.length));
  const lastHeadNl = head.lastIndexOf('\n');
  if (lastHeadNl > 0) head = head.slice(0, lastHeadNl + 1);

  // Tail: last tailBudget chars, snapped to the following line boundary
  let tail = str.slice(Math.max(0, str.length - tailBudget));
  const firstTailNl = tail.indexOf('\n');
  if (firstTailNl > 0) tail = tail.slice(firstTailNl + 1);

  const headLen = head.length;
  const tailLen = tail.length;

  if (keyLines.length > 0) {
    let keyText = keyLines.join('\n');
    const marker = `\n\n[... ${str.length - headLen - keyText.length - tailLen} characters truncated (middle omitted) ...]\n\n`;
    const availableForKey = budget - headLen - tailLen - marker.length;

    if (availableForKey > 0) {
      if (keyText.length > availableForKey) {
        keyText = keyText.slice(0, Math.max(availableForKey - 60, 0))
          + `\n[... ${keyLines.length} error lines truncated ...]`;
      }
      return `${head}${marker}${keyText}\n\n${tail}`;
    }
  }

  // --- 4. Fallback: simple head + tail ---
  const omitted = str.length - headLen - tailLen;
  return `${head}\n\n[... ${omitted} characters truncated (middle omitted) ...]\n\n${tail}`;
}

/** Render parallel sub-agent results into a single string for the main agent. */
export function formatParallelResults(results: ParallelResult[], agentNames: string[]): string {
  const lines: string[] = [`## Parallel results (${results.length} tasks)`, ''];
  results.forEach((r, i) => {
    const name = agentNames[i] ?? 'default';
    if (r.success) {
      lines.push(`### Task ${i} — ${name} — success (${r.iterations} iterations)`);
      lines.push(r.content ?? '');
    } else {
      lines.push(`### Task ${i} — ${name} — FAILED`);
      lines.push(r.error ?? 'unknown error');
    }
    lines.push('');
  });
  return lines.join('\n').trimEnd();
}

/** Truncate a parsed JSON value below budget while preserving structure. */
function jsonTruncate(obj: unknown, budget: number): string {
  if (Array.isArray(obj)) {
    if (obj.length <= 5) {
      const s = JSON.stringify(obj);
      return s.length <= budget ? s : s.slice(0, Math.max(budget - 3, 0)) + '...';
    }
    const kept = obj.slice(0, Math.min(3, obj.length));
    kept.push(`... ${obj.length - 3} more items ...`);
    const s = JSON.stringify(kept);
    if (s.length <= budget) return s;
    return JSON.stringify([obj[0], `... ${obj.length - 1} more items ...`]);
  }
  if (typeof obj === 'object' && obj !== null) {
    const entries = Object.entries(obj);
    const processed: Record<string, unknown> = {};
    for (const [k, v] of entries) {
      processed[k] = Array.isArray(v) && v.length > 5
        ? [v[0], `... ${v.length - 1} more items ...`]
        : typeof v === 'string' && v.length > 500
          ? v.slice(0, 500) + '...'
          : v;
    }
    const s = JSON.stringify(processed);
    if (s.length <= budget) return s;
    // Still too large — keep first 10 keys only
    const firstKeys = entries.slice(0, 10);
    const minimal: Record<string, unknown> = {};
    for (const [k] of firstKeys) {
      minimal[k] = processed[k];
    }
    if (entries.length > 10) minimal['...'] = `${entries.length - 10} more keys`;
    const s2 = JSON.stringify(minimal);
    if (s2.length <= budget) return s2;
    return s2.slice(0, Math.max(budget - 3, 0)) + '...';
  }
  const s = JSON.stringify(obj);
  return s.length <= budget ? s : s.slice(0, Math.max(budget - 3, 0)) + '...';
}

/**
 * Orchestrates a single user→assistant turn with multi-turn tool execution.
 *
 * Per DEVELOPMENT_GUIDE §6.1:
 * 1. Append user message to history
 * 2. Call LLM (streaming) with tools if executor is available
 * 3. Collect response text + tool_calls
 * 4. If tool_calls → permission gate → execute → append results → loop to 2
 * 5. If no tool_calls → done
 *
 * Usage from a React component:
 *
 * ```ts
 * const { streamChat, cancel } = useChatStream();
 * const loop = new AgentLoop(streamChat, cancel, toolExecutor);
 * loop.run("Hello", callbacks, { sessionId, messages, providerId, model });
 * ```
 */
export class AgentLoop {
  private readonly streamChat: StreamChatFn;
  private readonly cancelFn: CancelFn;
  private readonly toolExecutor: ToolExecutor | null;
  private readonly streamFactory: StreamChatFactory | null;
  private toolIterationLimit: number;
  /** Set by cancel() to break the multi-turn loop. Aborting the current stream
   *  alone is not enough — the for-loop would otherwise start the next turn. */
  private cancelled = false;
  /** AbortController for in-flight tool executions. Signaled by cancel() so
   *  long-running tools (e.g. run_command) don't block the stop button. */
  private toolAbortController: AbortController | null = null;
  private failedToolCalls = new Map<string, { count: number; lastArgs: string; lastError: string }>();

  /** Cache for read-only tool results. Key = `${toolName}:${arguments}` */
  private toolResultCache = new Map<string, { content: string; timestamp: number }>();

  /** Tracks in-flight cacheable tool executions for parallel dedup. */
  private inFlightCache = new Map<string, Promise<string>>();

  /** Resolver functions for in-flight cache entries, indexed by cache key. */
  private inFlightResolvers = new Map<string, (content: string) => void>();

  /** TTL for cached tool results in milliseconds */
  private static readonly CACHE_TTL_MS = 30_000;

  /** Tools whose results are safe to cache (read-only, no side effects) */
  private static readonly CACHEABLE_TOOLS = new Set([
    'read_file', 'list_dir', 'grep', 'glob',
    'git_status', 'git_diff', 'git_diff_staged', 'git_log',
    'memory_read', 'agents_list', 'agents_rules_read',
    'skills_list',
  ]);

  /** Static model registry — populated from providerStore defaults for model routing. */
  private static modelRegistry: Map<string, string[]> = new Map([
    ['meyatu', ['openai/gpt-4o', 'deepseek-v4-pro', 'vertex/claude-sonnet-4-6', 'coding/gemini-2.5-pro']],
    ['openai-compatible', ['gpt-4o', 'gpt-4o-mini', 'gpt-4-turbo', 'gpt-3.5-turbo']],
    ['anthropic', ['claude-sonnet-4-20250514', 'claude-3-5-sonnet-20241022', 'claude-3-opus-20240229']],
    ['google', ['gemini-2.5-pro', 'gemini-2.5-flash', 'gemini-2.0-flash']],
  ]);

  constructor(
    streamChat: StreamChatFn,
    cancel: CancelFn,
    toolExecutor?: ToolExecutor,
    streamFactory?: StreamChatFactory,
  ) {
    this.streamChat = streamChat;
    this.cancelFn = cancel;
    this.toolExecutor = toolExecutor ?? null;
    this.toolIterationLimit = FALLBACK_MAX_TOOL_ITERATIONS;
    this.streamFactory = streamFactory ?? null;
  }

  private classifyIntent(userMessage: string, previousToolCallCount: number): 'simple' | 'complex' | 'default' {
    const msg = userMessage.trim();
    if (/analy[sz]e|design|architect|refactor|debug|investigate|explain.*complex/i.test(msg) || previousToolCallCount >= 3) return 'complex';
    if (msg.length < 100 && previousToolCallCount === 0) return 'simple';
    return 'default';
  }

  private getAvailableModels(providerId: string): string[] {
    return AgentLoop.modelRegistry.get(providerId) ?? [];
  }

  private findModelInTier(providerId: string, tier: ModelTier): string | null {
    const allModels = this.getAvailableModels(providerId);
    for (const model of allModels) {
      if (classifyModelTier(model) === tier) return model;
    }
    return null;
  }

  /**
   * Start a chat turn with multi-turn tool execution.
   *
   * The model drives the work: which tools to call, whether/how to verify, and
   * how to recover from errors are all its decisions (steered by the system
   * prompt). The framework only enforces guardrails — permission for
   * side-effecting tools, output truncation, an iteration cap, and cancellation.
   */
  async run(
    userContent: string | ContentPart[],
    callbacks: AgentCallbacks,
    context: AgentContext,
  ): Promise<void> {
    const messages: Message[] = [...context.messages];
    let errorInjected = false;
    this.toolIterationLimit = context.maxIterations ?? FALLBACK_MAX_TOOL_ITERATIONS;
    this.cancelled = false;

    const userMsg: Message = {
      id: crypto.randomUUID(),
      role: 'user',
      content: userContent,
      timestamp: Date.now(),
    };
    messages.push(userMsg);
    const userMessage = getTextContent(userContent);

    callbacks.onStateChange?.('thinking', 'Starting...');

    // Fire before_chat hooks
    await this.runHooks('before_chat', context, callbacks);

    // Load AGENTS.md rules from .meyatu/agents.yml and inject into system prompt
    if (this.toolExecutor) {
      try {
        const rulesOutput = await this.toolExecutor.execute({
          id: `rules-${crypto.randomUUID()}`,
          name: 'agents_rules_read',
          arguments: JSON.stringify({ path: '.' }),
        });
        if (rulesOutput?.content) {
          const parsed = JSON.parse(rulesOutput.content);
          const rulesPrompt: string = parsed.system_prompt ?? '';
          if (rulesPrompt) {
            context = {
              ...context,
              systemPrompt: context.systemPrompt
                ? `${rulesPrompt}\n\n${context.systemPrompt}`
                : rulesPrompt,
            };
            // Apply execution config overrides if present
            const exec = parsed.execution;
            if (exec?.max_iterations && exec.max_iterations > 0 && exec.max_iterations <= 100) {
              this.toolIterationLimit = exec.max_iterations;
            }
          }
        }
      } catch {
        // Best-effort: missing or invalid agents.yml is not fatal
      }

      // Load project memory (.meyatu/memory.md) so the model starts each session
      // knowing what it learned before — the substrate for self-improvement
      // across sessions. The model also writes to it via memory_write.
      try {
        const mem = await this.toolExecutor.execute({
          id: `mem-${crypto.randomUUID()}`,
          name: 'memory_read',
          arguments: JSON.stringify({ path: '.' }),
        });
        let memText = (mem?.content ?? '').trim();
        if (memText) {
          // Cap what we inject every turn so a large memory file can't blow up
          // the prompt; the model can still memory_read the full file on demand.
          const MEMORY_INJECT_CAP = 6000;
          if (memText.length > MEMORY_INJECT_CAP) {
            memText = `${memText.slice(0, MEMORY_INJECT_CAP)}\n…[memory truncated — use memory_read for the full file, and consider memory_write mode "replace" to trim it]`;
          }
          const memBlock = `PROJECT MEMORY — notes you recorded in earlier sessions (conventions, gotchas, the user's preferences, fixes for recurring problems). Treat these as known facts and keep them updated via memory_write:\n\n${memText}`;
          context = {
            ...context,
            systemPrompt: context.systemPrompt
              ? `${context.systemPrompt}\n\n${memBlock}`
              : memBlock,
          };
        }
      } catch {
        // Best-effort: missing memory is not fatal
      }
    }

    // Model routing: classify intent and potentially swap to a faster/stronger model
    const previousToolCallCount = messages.filter(m => m.role === 'assistant' && m.toolCalls?.length).length;
    const intent = this.classifyIntent(userMessage, previousToolCallCount);
    let routedModel = context.model;

    if (intent === 'simple') {
      const currentTier = classifyModelTier(context.model);
      if (currentTier !== 'fast') {
        const fastModel = this.findModelInTier(context.providerId, 'fast');
        if (fastModel) {
          routedModel = fastModel;
          callbacks.onStateChange?.('routing', `Using fast model for simple query`);
        }
      }
    } else if (intent === 'complex') {
      const currentTier = classifyModelTier(context.model);
      if (currentTier !== 'strong') {
        const strongModel = this.findModelInTier(context.providerId, 'strong');
        if (strongModel) {
          routedModel = strongModel;
          callbacks.onStateChange?.('routing', `Using strong model for complex task`);
        }
      }
    }

    try {
      for (let iteration = 0; iteration < this.toolIterationLimit; iteration++) {
        if (this.cancelled) {
          callbacks.onStateChange?.('done');
          callbacks.onDone?.();
          return;
        }
        const request = this.buildRequest(messages, context, routedModel);
        callbacks.onStateChange?.('thinking', 'Calling LLM...');

        let streamResult = await this.streamSingleTurn(request, callbacks);

        // Fallback retry on provider errors (429/5xx) — max 1 retry with a lower-tier model
        if (streamResult.fallbackEligibleError && streamResult.content.length === 0 && streamResult.toolCalls.length === 0) {
          const fallbackModel = getFallbackModel(routedModel, this.getAvailableModels(context.providerId));
          if (fallbackModel) {
            callbacks.onStateChange?.('fallback', `Switching to ${fallbackModel} after provider error`);
            const fallbackRequest = this.buildRequest(messages, context, fallbackModel);
            streamResult = await this.streamSingleTurn(fallbackRequest, callbacks);
            routedModel = fallbackModel;
          }
        }

        const { content, reasoningContent, toolCalls } = streamResult;

        // Cancelled mid-stream → stop without starting another turn.
        if (this.cancelled) {
          finalizeTurn(messages, content, [], reasoningContent);
          callbacks.onStateChange?.('done');
          callbacks.onDone?.();
          return;
        }

        if (toolCalls.length === 0) {
          // The model produced a final answer with no more tool calls. Whether
          // it verified its work (ran tests, a build, etc.) is its own decision,
          // guided by the system prompt — the framework doesn't force it.
          finalizeTurn(messages, content, [], reasoningContent);
          callbacks.onStateChange?.('done');
          await this.runHooks('after_chat', context, callbacks).catch(() => {});
          callbacks.onDone?.();
          return;
        }

        // Tool calls present → add assistant message with tool calls
        callbacks.onStateChange?.('tool_call', toolCalls[0].name);
        finalizeTurn(messages, content, toolCalls, reasoningContent);

        // Execute tools in parallel — each tool is independent.
        // Create a fresh AbortController so cancel() can interrupt in-flight tools.
        this.toolAbortController = new AbortController();
        const toolSignal = this.toolAbortController.signal;
        toolCalls.forEach(tc => callbacks.onToolProgress?.({ toolName: tc.name, toolCallId: tc.id, status: 'starting' }));
        const settled = await Promise.allSettled(
          toolCalls.map(tc => {
            if (this.cancelled) return Promise.resolve({ toolCallId: tc.id, content: 'Cancelled.' });
            // Race the tool execution against a cancellation poll so the
            // stop button / softCancel doesn't block on a hung tool.
            return Promise.race([
              this.executeOrDispatch(tc, callbacks, context, toolSignal),
              new Promise<ToolResult>(resolve => {
                const poll = () => {
                  if (this.cancelled) {
                    resolve({ toolCallId: tc.id, content: 'Cancelled.' });
                  } else {
                    setTimeout(poll, 100);
                  }
                };
                poll();
              }),
            ]);
          }),
        );
        this.toolAbortController = null;
        // Process results in order, pushing to messages
        for (let i = 0; i < toolCalls.length; i++) {
          if (this.cancelled) break;
          const outcome = settled[i];
          const tc = toolCalls[i];
          if (outcome.status === 'fulfilled') {
            callbacks.onToolProgress?.({ toolName: tc.name, toolCallId: tc.id, status: 'completed' });
            messages.push({
              id: crypto.randomUUID(),
              role: 'tool',
              content: outcome.value.content,
              toolCallId: tc.id,
              name: tc.name,
              timestamp: Date.now(),
            });
            callbacks.onToolResult?.(outcome.value);
          } else {
            // Tool threw an unhandled error — surface as error result
            const errMsg = outcome.reason instanceof Error ? outcome.reason.message : String(outcome.reason);
            callbacks.onToolProgress?.({ toolName: tc.name, toolCallId: tc.id, status: 'failed', error: errMsg });
            messages.push({
              id: crypto.randomUUID(),
              role: 'tool',
              content: `Tool '${tc.name}' failed with error: ${errMsg}`,
              toolCallId: tc.id,
              name: tc.name,
              timestamp: Date.now(),
            });
          }
        }
        // Loop back to call LLM again with tool results (unless cancelled)
        if (this.cancelled) {
          callbacks.onStateChange?.('done');
          callbacks.onDone?.();
          return;
        }
      }

      // Hit the iteration cap (a cost backstop). Surface it factually and let
      // the model decide how to respond — no prescribed template.
      messages.push({
        id: crypto.randomUUID(),
        role: 'user',
        content: `Reached the ${this.toolIterationLimit}-iteration limit for this turn. Summarize what you accomplished and what remains.`,
        timestamp: Date.now(),
      });
      errorInjected = true;
    } catch (err: unknown) {
      const message = err instanceof Error ? err.message : String(err);
      // Surface the raw error to the model as context and let it respond.
      messages.push({
        id: crypto.randomUUID(),
        role: 'user',
        content: `An error occurred while running the task: ${message}`,
        name: 'agent_error',
        timestamp: Date.now(),
      });
      errorInjected = true;
    }

    // If an error was injected, let the LLM analyze and respond
    if (errorInjected) {
      callbacks.onStateChange?.('thinking', 'Analyzing error…');
      try {
        const analysisRequest = this.buildRequest(messages, context, routedModel);
        const { content: analysis, reasoningContent: analysisReasoning, toolCalls: analysisTcs } = await this.streamSingleTurn(
          analysisRequest,
          callbacks,
        );
        // If analysis produced tool calls (LLM trying to fix), give them one shot
        if (analysisTcs.length > 0) {
          // Execute analysis tool calls in parallel
          analysisTcs.forEach(tc => callbacks.onToolProgress?.({ toolName: tc.name, toolCallId: tc.id, status: 'starting' }));
          const analysisSettled = await Promise.allSettled(
            analysisTcs.map(tc =>
              Promise.race([
                this.executeToolWithPermission(tc, callbacks),
                new Promise<ToolResult>(resolve => {
                  const poll = () => {
                    if (this.cancelled) resolve({ toolCallId: tc.id, content: 'Cancelled.' });
                    else setTimeout(poll, 100);
                  };
                  poll();
                }),
              ]),
            ),
          );
          for (let i = 0; i < analysisTcs.length; i++) {
            const outcome = analysisSettled[i];
            const tc = analysisTcs[i];
            if (outcome.status === 'fulfilled') {
              callbacks.onToolProgress?.({ toolName: tc.name, toolCallId: tc.id, status: 'completed' });
              messages.push({
                id: crypto.randomUUID(),
                role: 'tool',
                content: outcome.value.content,
                toolCallId: tc.id,
                name: tc.name,
                timestamp: Date.now(),
              });
            } else {
              const errMsg = outcome.reason instanceof Error ? outcome.reason.message : String(outcome.reason);
              callbacks.onToolProgress?.({ toolName: tc.name, toolCallId: tc.id, status: 'failed', error: errMsg });
              messages.push({
                id: crypto.randomUUID(),
                role: 'tool',
                content: `Tool '${tc.name}' failed with error: ${errMsg}`,
                toolCallId: tc.id,
                name: tc.name,
                timestamp: Date.now(),
              });
            }
          }
          // Give LLM one more chance to respond with the tool results
          const finalReq = this.buildRequest(messages, context, routedModel);
          const { content: finalContent, reasoningContent: finalReasoning } = await this.streamSingleTurn(finalReq, callbacks);
          finalizeTurn(messages, finalContent, [], finalReasoning);
        } else {
          finalizeTurn(messages, analysis, [], analysisReasoning);
        }
        callbacks.onStateChange?.('done');
      } catch {
        // If even the analysis fails, fallback to original onError
        callbacks.onError?.('ERROR', 'Failed to process error analysis');
      }
    }

    // Fire after_chat hooks (best-effort, fire even on error)
    await this.runHooks('after_chat', context, callbacks).catch(() => {});
    callbacks.onDone?.();
  }

  /** Autonomous development mode: plan → execute → verify → auto-PR.
   *
   * Wraps the normal agent loop with a planning-augmented system prompt
   * that instructs the LLM to decompose the task, execute step by step,
   * run verification, and optionally create a GitHub PR.
   */
  async runAutonomous(
    userMessage: string,
    callbacks: AgentCallbacks,
    context: AgentContext,
  ): Promise<void> {
    const planPrompt = `You are an autonomous developer operating without hand-holding. Your job is to take the task from start to finish with no user intervention. Follow this process:

1. **Plan**: Analyze the task. Break it into clear steps. Create or update a plan file (.meyatu/task-plan.json) tracking each step with id, description, and status ("pending" → "in_progress" → "completed" or "failed").
2. **Execute & Verify**: Work through the steps one at a time. **After EACH step that modifies code**, immediately run verification:
   - TypeScript changes: \`npx tsc --noEmit\` (fast type check)
   - Rust changes: \`cargo check\` (fast compilation check)
   - If verification fails, fix the issue before moving to the next step
   - Do NOT wait until all steps are done — verify incrementally
3. **Final Verification**: After ALL steps are done, run the full test suite: \`cargo test --lib && npx vitest run\`. Fix any failures.
4. **Record**: Write any durable lesson, convention discovered, or recurring fix pattern to project memory (memory_write).
5. **Commit**: Stage and commit with a descriptive message.${context.autoPr ? '\n6. **PR**: Create a single GitHub PR that summarizes all changes with gh_pr_create.' : ''}

Remember: you are autonomous. Verify after EACH step, not just at the end. Fix what breaks immediately, then continue.

Task: ${userMessage}`;

    const autoContext: AgentContext = {
      ...context,
      maxIterations: 50,
      systemPrompt: context.systemPrompt
        ? `${context.systemPrompt}\n\n${planPrompt}`
        : planPrompt,
    };

    callbacks.onPlanStep?.({
      id: 0,
      description: userMessage,
      status: 'pending',
    });

    await this.run(userMessage, callbacks, autoContext);

    if (context.autoPr && this.toolExecutor) {
      callbacks.onStateChange?.('autonomous', 'Creating PR...');
      try {
        const result = await this.toolExecutor.execute({
          id: 'auto-pr',
          name: 'gh_pr_create',
          arguments: JSON.stringify({
            path: '.',
            title: `Autonomous: ${userMessage.slice(0, 70)}`,
            body: `Automated PR for: ${userMessage}`,
          }),
        });
        callbacks.onPlanStep?.({
          id: -1,
          description: 'Create PR',
          status: 'completed',
          result: result.content,
        });
      } catch (e) {
        callbacks.onPlanStep?.({
          id: -1,
          description: 'Create PR',
          status: 'failed',
          result: e instanceof Error ? e.message : String(e),
        });
      }
    }
  }

  /** Build a ChatRequest from current messages + context.
   *
   *  IMPORTANT: Frontend Message.toolCalls uses a flat `{id, name, arguments}`
   *  format but the Rust adapter expects OpenAI format `{id, type, function: {name, arguments}}`.
   *  Sending toolCalls from history causes `missing field 'type'` deserialization errors.
   *  We strip them here — tool results (role="tool") are already in the message array,
   *  which is all the API needs to continue a conversation.
   */
  private buildRequest(messages: Message[], context: AgentContext, modelOverride?: string): ChatRequest {
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    type CleanMsg = { role: string; content: string | any[]; reasoningContent?: string; toolCallId?: string; name?: string; toolCalls?: any[] };

    // 1) Map to the wire shape. Crucially, do NOT invent fake tool_call ids
    //    (the old `unknown-${Date.now()}` produced a different id every call,
    //    so tool messages never matched their assistant tool_call → HTTP 400).
    const lastUserIdx = messages.map((m) => m.role).lastIndexOf('user');
    const MAX_TEXT_CONTENT_BYTES = 30_000;
    const MAX_IMAGE_CONTENT_BYTES = 4_000_000; // 4MB — covers base64 images within API limits
    const raw: CleanMsg[] = messages.map(({ role, content, toolCallId, name, toolCalls, reasoningContent }, i) => {
      let wireContent = content;
      if (Array.isArray(content) && i !== lastUserIdx) {
        wireContent = getTextContent(content);
      }
      const clean: CleanMsg = {
        role,
        content: wireContent,
      };
      if (reasoningContent) clean.reasoningContent = reasoningContent;
      if (toolCallId) clean.toolCallId = toolCallId;
      if (name) clean.name = name;
      if (role === 'assistant' && toolCalls && toolCalls.length > 0) {
        clean.toolCalls = toolCalls
          .filter((tc) => tc && tc.id)
          .map((tc) => ({
            id: tc.id,
            type: 'function',
            function: { name: tc.name, arguments: tc.arguments },
          }));
      }
      return clean;
    });

    for (let i = 0; i < raw.length; i++) {
      const m = raw[i];
      if (Array.isArray(m.content)) {
        const hasImages = m.content.some(
          // eslint-disable-next-line @typescript-eslint/no-explicit-any
          (p: any) => p && p.type === 'image_url' && p.image_url?.url,
        );
        const limit = hasImages ? MAX_IMAGE_CONTENT_BYTES : MAX_TEXT_CONTENT_BYTES;
        const serialized = JSON.stringify(m.content);
        if (serialized.length > limit) {
          raw[i] = { ...m, content: getTextContent(m.content) };
        }
      }
    }

    // 2) Sanitize so the sequence is ALWAYS valid for the API, even when the
    //    persisted history is broken (e.g. old sessions saved before the
    //    toolCallId fix). Two OpenAI rules to satisfy:
    //      a) every assistant tool_call must have a following tool response;
    //      b) every tool message must respond to a real assistant tool_call.
    const respondedIds = new Set(
      raw.filter((m) => m.role === 'tool' && m.toolCallId).map((m) => m.toolCallId as string),
    );
    const assistantCallIds = new Set<string>();
    for (const m of raw) {
      if (m.role === 'assistant' && m.toolCalls) {
        for (const tc of m.toolCalls) assistantCallIds.add(tc.id);
      }
    }
    const cleanMessages: CleanMsg[] = [];
    for (const m of raw) {
      if (m.role === 'tool') {
        // Keep only tool messages that answer a known assistant tool_call.
        if (m.toolCallId && assistantCallIds.has(m.toolCallId)) cleanMessages.push(m);
        continue;
      }
      if (m.role === 'assistant' && m.toolCalls) {
        // Keep only tool_calls that actually got a response; drop the rest so
        // the assistant message never has an unanswered tool_call.
        const kept = m.toolCalls.filter((tc) => respondedIds.has(tc.id));
        if (kept.length > 0) {
          cleanMessages.push({ ...m, toolCalls: kept });
        } else {
          // Unanswered tool_calls → keep the message but without toolCalls.
          const stripped: CleanMsg = { role: m.role, content: m.content };
          if (m.toolCallId) stripped.toolCallId = m.toolCallId;
          if (m.name) stripped.name = m.name;
          cleanMessages.push(stripped);
        }
        continue;
      }
      cleanMessages.push(m);
    }

    // 3) Compress early history if conversation is long
    const compressedMessages = this.compressHistory(cleanMessages);

    const request: ChatRequest = {
      sessionId: context.sessionId,
      messages: compressedMessages,
      providerId: context.providerId,
      model: modelOverride ?? context.model,
    };
    if (context.systemPrompt !== undefined) {
      request.systemPrompt = context.systemPrompt;
    }
    if (context.baseUrl !== undefined) {
      request.baseUrl = context.baseUrl;
    }
    if (this.toolExecutor) {
      // Wrap each tool in OpenAI function-calling format. All provider
      // adapters expect `{ type: "function", function: {...} }`; sending the
      // bare definition makes the API reject it ("unknown variant").
      request.tools = this.toolExecutor.getDefinitions().map((fn) => ({
        type: 'function' as const,
        function: fn,
      }));
    }
    return request;
  }

  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  private compressHistory<T extends { role: string; content: string | any[] }>(messages: T[]): T[] {
    const COMPRESS_THRESHOLD = 30;
    const TOOL_CONTENT_LIMIT = 1000;

    if (messages.length <= COMPRESS_THRESHOLD) return messages;

    // Start at the midpoint, then slide backward to find a safe split.
    // A safe split means: the message before the split is NOT an assistant
    // message with tool_calls whose paired tool responses start right after.
    // The OpenAI API requires tool responses to immediately follow the
    // assistant message that made the tool call — inserting a user message
    // between them causes a 400 "insufficient tool messages" error.
    let splitPoint = Math.floor(messages.length / 2);

    // Collect all tool_call IDs from assistant messages and their responding
    // tool message IDs to detect paired sequences.
    type SeqMsg = { role?: string; toolCalls?: Array<{ id?: string }>; toolCallId?: string };
    while (splitPoint > 1) {
      const prev = messages[splitPoint - 1] as unknown as SeqMsg | undefined;
      const next = messages[splitPoint] as unknown as SeqMsg | undefined;

      // Check if the message just before the split is an assistant with tool_calls.
      if (prev && prev.role === 'assistant' && Array.isArray(prev.toolCalls) && prev.toolCalls.length > 0) {
        // Get the set of tool_call IDs from this assistant message.
        const pendingIds = new Set(prev.toolCalls.map((tc) => tc.id).filter(Boolean));

        // If the next message is a tool response for one of these calls,
        // the split is unsafe — slide one position earlier.
        if (next && next.role === 'tool' && next.toolCallId && pendingIds.has(next.toolCallId)) {
          splitPoint--;
          continue;
        }
      }

      // Also check: is the message just before the split a tool message
      // that answers an assistant message earlier in the early half?
      // This is safe — the assistant+tool pair is both in the early half.
      break;
    }

    // Ensure we don't split too aggressively — keep at least 4 messages
    // in the recent half so the API has enough context.
    if (splitPoint > messages.length - 4) {
      splitPoint = messages.length - 4;
    }

    const early = messages.slice(0, splitPoint);
    const recent = messages.slice(splitPoint);

    const compressed = early.map((msg) => {
      // Only truncate plain-string tool results; multimodal content is left intact.
      if (msg.role === 'tool' && typeof msg.content === 'string' && msg.content.length > TOOL_CONTENT_LIMIT) {
        return { ...msg, content: msg.content.slice(0, TOOL_CONTENT_LIMIT) + '\n[...truncated]' };
      }
      return msg;
    });

    const summaryMarker = {
      role: 'user',
      content: '[Earlier conversation history compressed. Tool outputs truncated to key content.]',
    } as T;

    return [...compressed, summaryMarker, ...recent];
  }

  /** Stream a single LLM call and collect content + tool calls. */
  private async streamSingleTurn(
    request: ChatRequest,
    callbacks: AgentCallbacks,
  ): Promise<{ content: string; reasoningContent: string; toolCalls: ToolCall[]; fallbackEligibleError: boolean }> {
    let content = '';
    let reasoningContent = '';
    const toolCallMap = new Map<string, ToolCall>();
    let fallbackEligibleError = false;

    const generator = this.streamChat(request);
    for await (const event of generator) {
      switch (event.type) {
        case 'content_delta':
          content += event.content;
          callbacks.onDelta?.(event.content);
          break;
        case 'reasoning_delta':
          reasoningContent += event.content;
          break;
        case 'tool_call':
          {
            const tc: ToolCall = {
              id: event.id,
              name: event.name,
              arguments: event.arguments,
            };
            // Backend sends cumulative arguments (Rust BTreeMap accumulation),
            // so we REPLACE rather than append to avoid JSON corruption.
            const existing = toolCallMap.get(event.id);
            if (existing) {
              existing.arguments = event.arguments;
            } else {
              toolCallMap.set(event.id, tc);
              callbacks.onToolCall?.(tc);
            }
          }
          break;
        case 'error':
          if (/429|5[0-9]{2}|RATE_LIMIT|OVERLOADED|TIMEOUT|rate.limit|overloaded/i.test(event.code + ' ' + event.message)) {
            fallbackEligibleError = true;
          }
          callbacks.onError?.(event.code, event.message, event.retryable);
          break;
        case 'done':
          if (event.usage) {
            callbacks.onTokens?.(event.usage);
          }
          break;
        case 'tool_result':
          // tool_result events from previous tool executions in the stream;
          // we don't need to re-dispatch since we emit our own ToolResult
          break;
      }
    }

    return { content, reasoningContent, toolCalls: Array.from(toolCallMap.values()), fallbackEligibleError };
  }

  /** Check permission gate (if needed), then execute the tool. */
  private async executeToolWithPermission(
    tc: ToolCall,
    callbacks: AgentCallbacks,
    _signal?: AbortSignal,
  ): Promise<ToolResult> {
    // Cache check — return cached result for read-only tools if fresh
    let cacheKeyForCleanup: string | undefined;
    if (AgentLoop.CACHEABLE_TOOLS.has(tc.name)) {
      const cacheKey = `${tc.name}:${tc.arguments}`;
      cacheKeyForCleanup = cacheKey;

      // Check resolved cache
      const cached = this.toolResultCache.get(cacheKey);
      if (cached && Date.now() - cached.timestamp < AgentLoop.CACHE_TTL_MS) {
        return { toolCallId: tc.id, content: cached.content };
      }

      // Check in-flight: another identical call is already executing
      const inFlight = this.inFlightCache.get(cacheKey);
      if (inFlight) {
        const content = await inFlight;
        return { toolCallId: tc.id, content };
      }

      // Mark as in-flight before any async gaps (hooks, permissions)
      let resolveInFlight: (content: string) => void;
      const inFlightPromise = new Promise<string>((resolve) => {
        resolveInFlight = resolve;
      });
      this.inFlightCache.set(cacheKey, inFlightPromise);
      this.inFlightResolvers.set(cacheKey, resolveInFlight!);
    }

    // Skip hooks for internal tools (hooks_run itself)
    if (tc.name !== 'hooks_run') {
      await this.runToolHook('before_tool', tc, undefined, undefined, callbacks);
    }

    // Permission gate: side-effecting tools need the user's consent. This is a
    // safety guardrail, not a decision about how to solve the task. If denied,
    // report it factually and let the model decide what to do next (the system
    // prompt tells it not to retry the same call).
    if (this.toolExecutor?.hasSideEffect(tc.name)) {
      if (!callbacks.onPermissionRequest) {
        this.cleanupInFlight(cacheKeyForCleanup);
        return {
          toolCallId: tc.id,
          content: `'${tc.name}' was not run: no permission handler is available to approve side-effecting tools.`,
        };
      }
      const decision = await callbacks.onPermissionRequest(tc);

      if (decision === 'deny') {
        this.cleanupInFlight(cacheKeyForCleanup);
        return {
          toolCallId: tc.id,
          content: `The user denied permission to run '${tc.name}'.`,
        };
      }
      // 'allow' and 'always_allow' both proceed
    }

    // Execute the tool
    if (this.toolExecutor) {
      const toolKey = `${tc.name}:${tc.arguments}`;
      const failureRecord = this.failedToolCalls.get(toolKey);
      if (failureRecord && failureRecord.count >= 2) {
        this.cleanupInFlight(cacheKeyForCleanup);
        return {
          toolCallId: tc.id,
          content: `CIRCUIT BREAKER: '${tc.name}' with these arguments has failed ${failureRecord.count} times. Last error: ${failureRecord.lastError}. Try a different approach or different arguments.`,
        };
      }

      const maxRetries = 2;
      const isSideEffect = this.toolExecutor?.hasSideEffect(tc.name) ?? false;

      for (let attempt = 0; attempt <= maxRetries; attempt++) {
        try {
          const result = await this.toolExecutor.execute(tc);
          result.content = truncateToolResult(result.content);
          if (tc.name !== 'hooks_run') {
            await this.runToolHook('after_tool', tc, result.content, undefined, callbacks);
          }
          this.failedToolCalls.delete(toolKey);

          // Store in cache for read-only tools
          if (AgentLoop.CACHEABLE_TOOLS.has(tc.name)) {
            const cacheKey = `${tc.name}:${tc.arguments}`;
            this.toolResultCache.set(cacheKey, { content: result.content, timestamp: Date.now() });
            // Resolve any parallel identical calls waiting on this result
            const resolveFn = this.inFlightResolvers.get(cacheKey);
            if (resolveFn) {
              resolveFn(result.content);
              this.inFlightResolvers.delete(cacheKey);
            }
            this.inFlightCache.delete(cacheKey);
          } else {
            // Invalidate cache on successful write operations
            this.toolResultCache.clear();
            this.inFlightResolvers.clear();
            this.inFlightCache.clear();

            // Post-edit auto-test: fire-and-forget test run after file edits
            if ((tc.name === 'write_file' || tc.name === 'edit_file') && this.toolExecutor) {
              const srcExts = ['.ts', '.tsx', '.js', '.jsx', '.rs', '.py', '.go', '.java', '.kt'];
              let filePath = '';
              try { filePath = (JSON.parse(tc.arguments) as { path?: string }).path ?? ''; } catch { /* non-JSON args */ }
              const ext = filePath.substring(filePath.lastIndexOf('.'));
              if (srcExts.includes(ext)) {
                const testArgs = JSON.stringify({ path: '.' });
                const testCall: import('../types/message').ToolCall = {
                  id: `auto-test-${Date.now()}`,
                  name: 'test_runner',
                  arguments: testArgs,
                };
                this.toolExecutor.execute(testCall).then((testResult) => {
                  const parsed = JSON.parse(testResult.content);
                  if (!parsed.success && parsed.failures?.length > 0) {
                    const failSummary = parsed.failures
                      .slice(0, 3)
                      .map((f: { name: string; message: string }) => `${f.name}: ${f.message.substring(0, 100)}`)
                      .join('\n');
                    callbacks.onError?.('TEST_FAILED', `Auto-test after edit failed:\n${failSummary}`);
                  }
                }).catch(() => {});
              }
            }
          }

          return result;
        } catch (err: unknown) {
          const errorMsg = err instanceof Error ? err.message : String(err);

          const isTransient = /timeout|ECONNRESET|ECONNREFUSED|ETIMEDOUT|resource temporarily unavailable|socket hang up|rate limit|429/i.test(errorMsg);
          const canRetry = isTransient && (!isSideEffect || /ECONNREFUSED|ETIMEDOUT|timeout/i.test(errorMsg));

          if (canRetry && attempt < maxRetries) {
            await new Promise((resolve) => setTimeout(resolve, 100 * Math.pow(4, attempt)));
            continue;
          }

          if (tc.name !== 'hooks_run') {
            await this.runToolHook('after_tool', tc, undefined, errorMsg, callbacks);
          }

          const current = this.failedToolCalls.get(toolKey) ?? { count: 0, lastArgs: tc.arguments, lastError: '' };
          this.failedToolCalls.set(toolKey, {
            count: current.count + 1,
            lastArgs: tc.arguments,
            lastError: errorMsg,
          });

          this.cleanupInFlight(cacheKeyForCleanup);

          return {
            toolCallId: tc.id,
            content: `Tool execution error: ${errorMsg}`,
          };
        }
      }

      this.cleanupInFlight(cacheKeyForCleanup);

      return {
        toolCallId: tc.id,
        content: 'Tool execution failed after retries',
      };
    }

    this.cleanupInFlight(cacheKeyForCleanup);

    return {
      toolCallId: tc.id,
      content: 'No tool executor available',
    };
  }

  /** Fire lifecycle hooks for a given event. */
  private async runHooks(
    event: string,
    context: AgentContext,
    callbacks: AgentCallbacks,
  ): Promise<void> {
    if (!this.toolExecutor) return;
    try {
      const result = await this.toolExecutor.execute({
        id: `hook-${crypto.randomUUID()}`,
        name: 'hooks_run',
        arguments: JSON.stringify({
          path: '.',
          event,
          tool_name: null,
          tool_args: null,
          tool_result: null,
          tool_error: null,
          session_id: context.sessionId,
        }),
      });
      callbacks.onHookEvent?.(event, result.content);
    } catch {
      // Hooks are best-effort — never block the main flow
    }
  }

  /** Fire before/after tool hooks for a specific tool call. */
  private async runToolHook(
    event: string,
    tc: ToolCall,
    resultContent: string | undefined,
    errorMsg: string | undefined,
    callbacks: AgentCallbacks,
  ): Promise<void> {
    if (!this.toolExecutor) return;
    try {
      const hookResult = await this.toolExecutor.execute({
        id: `hook-${crypto.randomUUID()}`,
        name: 'hooks_run',
        arguments: JSON.stringify({
          path: '.',
          event,
          tool_name: tc.name,
          tool_args: tc.arguments,
          tool_result: resultContent ?? null,
          tool_error: errorMsg ?? null,
          session_id: null,
        }),
      });
      callbacks.onHookEvent?.(event, hookResult.content);
    } catch {
      // Best-effort
    }
  }

  /** Load name → system_prompt from .meyatu/agents.yml via the agents_list tool. */
  private async loadAgents(): Promise<Record<string, string>> {
    if (!this.toolExecutor) return {};
    try {
      const out = await this.toolExecutor.execute({
        id: `agents-${crypto.randomUUID()}`,
        name: 'agents_list',
        arguments: JSON.stringify({ path: '.' }),
      });
      const parsed = JSON.parse(out.content) as { agents?: Array<{ name?: string; system_prompt?: string }> };
      const map: Record<string, string> = {};
      for (const a of parsed.agents ?? []) {
        if (a?.name && a?.system_prompt) map[a.name] = a.system_prompt;
      }
      return map;
    } catch {
      return {};
    }
  }

  /** Handle a dispatch_parallel_agents tool call by running runParallel. */
  private async dispatchParallel(
    tc: ToolCall,
    callbacks: AgentCallbacks,
    context: AgentContext,
  ): Promise<ToolResult> {
    if (!this.streamFactory) {
      return { toolCallId: tc.id, content: 'Parallel dispatch is unavailable in this session.' };
    }
    let parsed: { tasks?: Array<{ task?: string; agent?: string }> };
    try {
      parsed = JSON.parse(tc.arguments);
    } catch {
      return { toolCallId: tc.id, content: 'Invalid dispatch_parallel_agents arguments (not JSON).' };
    }
    const raw = Array.isArray(parsed.tasks) ? parsed.tasks : [];
    if (raw.length === 0) {
      return { toolCallId: tc.id, content: 'No tasks provided to dispatch_parallel_agents.' };
    }
    const agentMap = await this.loadAgents();
    const agentNames = raw.map((t) => t.agent ?? 'default');
    const tasks: ParallelTask[] = raw.map((t, i) => ({
      task: String(t.task ?? ''),
      agentName: agentNames[i],
      systemPrompt: t.agent && agentMap[t.agent] ? agentMap[t.agent] : context.systemPrompt,
    }));
    const results = await this.runParallel(tasks, callbacks, context, this.streamFactory);
    return { toolCallId: tc.id, content: formatParallelResults(results, agentNames) };
  }

  /** Route a tool call: intercept dispatch_parallel_agents, else normal execution. */
  private async executeOrDispatch(
    tc: ToolCall,
    callbacks: AgentCallbacks,
    context: AgentContext,
    signal?: AbortSignal,
  ): Promise<ToolResult> {
    if (tc.name === 'dispatch_parallel_agents') {
      return this.dispatchParallel(tc, callbacks, context);
    }
    return this.executeToolWithPermission(tc, callbacks, signal);
  }

  /** Maximum time a single parallel sub-task is allowed to run before being
   *  timed out, in milliseconds. */
  private static readonly PARALLEL_TASK_TIMEOUT_MS = 120_000;

  /** Run multiple sub-tasks in parallel, each with its own multi-turn tool
   *  execution loop, independent message history, and isolated error handling.
   *  All tasks share the same `toolExecutor`. Returns one `ParallelResult` per
   *  task in the same order as the input array. */
  async runParallel(
    tasks: ParallelTask[],
    callbacks: AgentCallbacks,
    context: AgentContext,
    streamFactory: StreamChatFactory,
  ): Promise<ParallelResult[]> {
    const maxIter = context.maxIterations ?? FALLBACK_MAX_TOOL_ITERATIONS;

    const runners = tasks.map(async (task, index): Promise<ParallelResult> => {
      const taskId = `parallel-${index}`;
      const agentName = task.agentName ?? 'default';
      callbacks.onParallelEvent?.({ taskId, agentName, phase: 'start' });
      const finish = (r: ParallelResult): ParallelResult => {
        callbacks.onParallelEvent?.({
          taskId,
          agentName,
          phase: r.success ? 'done' : 'error',
          summary: r.success ? (r.content ?? '').slice(0, 200) : undefined,
          error: r.success ? undefined : r.error,
          iteration: r.iterations,
        });
        return r;
      };
      const messages: Message[] = [
        {
          id: crypto.randomUUID(),
          role: 'user',
          content: task.task,
          timestamp: Date.now(),
        },
      ];

      const taskCallbacks: AgentCallbacks = {
        ...callbacks,
        onDelta: (content: string) => callbacks.onDelta?.(content),
        onToolCall: (tc: ToolCall) => callbacks.onToolCall?.(tc),
        onToolResult: (result: ToolResult) => {
          callbacks.onToolResult?.(result);
          callbacks.onStateChange?.('tool_result', `${taskId}: ${result.toolCallId}`);
        },
        onPermissionRequest: callbacks.onPermissionRequest
          ? (tc: ToolCall) => {
              callbacks.onStateChange?.('permission', `${taskId}: ${tc.name}`);
              return callbacks.onPermissionRequest!(tc);
            }
          : undefined,
        onStateChange: (state: string, detail?: string) =>
          callbacks.onStateChange?.(state, `[${taskId}] ${detail ?? ''}`),
        onTokens: (tokens: TokenUsage) => callbacks.onTokens?.(tokens),
        onError: (code: string, message: string, retryable?: boolean) => callbacks.onError?.(code, message, retryable),
        onDone: undefined,
        onHookEvent: callbacks.onHookEvent,
        onPlanStep: callbacks.onPlanStep,
        onToolProgress: (info: ToolProgressInfo) => callbacks.onToolProgress?.(info),
      };

      let iterations = 0;
      const stream = streamFactory();
      const deadline = Date.now() + AgentLoop.PARALLEL_TASK_TIMEOUT_MS;

      try {
        for (iterations = 0; iterations < maxIter; iterations++) {
          if (Date.now() > deadline) {
            return finish({
              taskId,
              success: false,
              error: `Task timed out after ${AgentLoop.PARALLEL_TASK_TIMEOUT_MS / 1000}s`,
              iterations,
            });
          }

          const request: ChatRequest = {
            sessionId: context.sessionId,
            messages,
            providerId: context.providerId,
            model: context.model,
          };
          if (task.systemPrompt || context.systemPrompt) {
            request.systemPrompt = task.systemPrompt ?? context.systemPrompt;
          }
          if (this.toolExecutor) {
            request.tools = this.toolExecutor.getDefinitions().map((fn) => ({
              type: 'function' as const,
              function: fn,
            }));
          }

          const { content, reasoningContent, toolCalls } = await this.streamWithFactory(
            stream,
            request,
            taskCallbacks,
          );

          if (toolCalls.length === 0) {
            finalizeTurn(messages, content, [], reasoningContent);
            return finish({ taskId, success: true, content, iterations });
          }

          finalizeTurn(messages, content, toolCalls, reasoningContent);
          taskCallbacks.onStateChange?.('tool_call', `${taskId}: ${toolCalls.map(t => t.name).join(', ')}`);
          callbacks.onParallelEvent?.({ taskId, agentName, phase: 'tool', tool: toolCalls[0]?.name, iteration: iterations });

          const settled = await Promise.allSettled(
            toolCalls.map((tc) =>
              Promise.race([
                this.executeToolWithPermission(tc, taskCallbacks),
                new Promise<ToolResult>(resolve => {
                  const poll = () => {
                    if (this.cancelled) resolve({ toolCallId: tc.id, content: 'Cancelled.' });
                    else setTimeout(poll, 100);
                  };
                  poll();
                }),
              ]),
            ),
          );

          for (let i = 0; i < toolCalls.length; i++) {
            const outcome = settled[i];
            const tc = toolCalls[i];
            if (outcome.status === 'fulfilled') {
              messages.push({
                id: crypto.randomUUID(),
                role: 'tool',
                content: outcome.value.content,
                toolCallId: tc.id,
                name: tc.name,
                timestamp: Date.now(),
              });
              taskCallbacks.onToolResult?.(outcome.value);
            } else {
              const errMsg =
                outcome.reason instanceof Error
                  ? outcome.reason.message
                  : String(outcome.reason);
              messages.push({
                id: crypto.randomUUID(),
                role: 'tool',
                content: `Tool '${tc.name}' failed with error: ${errMsg}`,
                toolCallId: tc.id,
                name: tc.name,
                timestamp: Date.now(),
              });
            }
          }
        }

        return finish({
          taskId,
          success: true,
          content: getTextContent(messages[messages.length - 1]?.content ?? ''),
          iterations,
        });
      } catch (err: unknown) {
        return finish({
          taskId,
          success: false,
          error: err instanceof Error ? err.message : String(err),
          iterations,
        });
      }
    });

    const settled = await Promise.allSettled(runners);
    return settled.map((s, i) => {
      if (s.status === 'fulfilled') return s.value;
      return {
        taskId: `parallel-${i}`,
        success: false,
        error: s.reason instanceof Error ? s.reason.message : String(s.reason),
        iterations: 0,
      };
    });
  }

  /** Stream a single LLM turn using the provided StreamChatFn instead of
   *  `this.streamChat`.  Identical to `streamSingleTurn` except it accepts
   *  a caller-supplied stream function so that parallel tasks can each use
   *  their own isolated stream. */
  private async streamWithFactory(
    streamFn: StreamChatFn,
    request: ChatRequest,
    callbacks: AgentCallbacks,
  ): Promise<{ content: string; reasoningContent: string; toolCalls: ToolCall[] }> {
    let content = '';
    let reasoningContent = '';
    const toolCallMap = new Map<string, ToolCall>();

    const generator = streamFn(request);
    for await (const event of generator) {
      switch (event.type) {
        case 'content_delta':
          content += event.content;
          callbacks.onDelta?.(event.content);
          break;
        case 'reasoning_delta':
          reasoningContent += event.content;
          break;
        case 'tool_call':
          {
            const tc: ToolCall = {
              id: event.id,
              name: event.name,
              arguments: event.arguments,
            };
            const existing = toolCallMap.get(event.id);
            if (existing) {
              existing.arguments = event.arguments;
            } else {
              toolCallMap.set(event.id, tc);
              callbacks.onToolCall?.(tc);
            }
          }
          break;
        case 'error':
          callbacks.onError?.(event.code, event.message, event.retryable);
          break;
        case 'done':
          if (event.usage) {
            callbacks.onTokens?.(event.usage);
          }
          break;
        case 'tool_result':
          break;
      }
    }

    return { content, reasoningContent, toolCalls: Array.from(toolCallMap.values()) };
  }

  /** Cancel the run: stop the multi-turn loop AND abort the in-flight stream.
   *  Setting `cancelled` makes the loop break before starting the next turn;
   *  `cancelFn()` aborts any stream currently in progress. Safe to call any time
   *  (e.g. while the loop is awaiting a permission decision — resolve that
   *  decision from the UI and the loop will then see `cancelled` and stop). */
  /** Clean up in-flight cache entry on error — prevents hanging promises. */
  private cleanupInFlight(cacheKey: string | undefined): void {
    if (!cacheKey) return;
    const resolveFn = this.inFlightResolvers.get(cacheKey);
    if (resolveFn) {
      resolveFn('Tool execution failed');
      this.inFlightResolvers.delete(cacheKey);
    }
    this.inFlightCache.delete(cacheKey);
  }

  cancel(): void {
    this.cancelled = true;
    this.cancelFn();
    if (this.toolAbortController) {
      this.toolAbortController.abort();
      this.toolAbortController = null;
    }
  }

  /** Soft cancel: set the cancelled flag so the loop breaks after the current
   *  iteration completes naturally. Does NOT abort the SSE stream or tools —
   *  the 100ms race poll picks up the flag. Used for 追问 — the pending chip
   *  stays visible while the current turn finishes, then the follow-up is
   *  dispatched via the agentBusy effect. */
  softCancel(): void {
    this.cancelled = true;
  }
}

/** Append the final assistant message to the evolving history. */
function finalizeTurn(
  messages: Message[],
  content: string,
  toolCalls: ToolCall[],
  reasoningContent?: string,
): void {
  messages.push({
    id: crypto.randomUUID(),
    role: 'assistant',
    content: content || '',
    ...(reasoningContent ? { reasoningContent } : {}),
    ...(toolCalls.length > 0 ? { toolCalls } : {}),
    timestamp: Date.now(),
  });
}
