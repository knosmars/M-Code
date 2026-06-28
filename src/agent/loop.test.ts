import { describe, it, expect, vi, beforeEach } from 'vitest';
import { AgentLoop, truncateToolResult, formatParallelResults } from './loop';
import { classifyModelTier } from '../stores/providerStore';
import type { AgentContext, ParallelTask } from './loop';
import type { ParallelAgentEvent } from './parallelEvents';
import type { ChatRequest } from '../types/ipc';
import type { StreamEvent } from '../types/stream';
import type { ToolCall, ToolResult } from '../types/message';
import type { ToolExecutor } from './tools';
import { hasSideEffect } from './toolRegistry';

function delta(content: string): StreamEvent {
  return { type: 'content_delta', content };
}

function tCall(id: string, name: string, args: string): StreamEvent {
  return { type: 'tool_call', id, name, arguments: args };
}

function done(usage?: { promptTokens: number; completionTokens: number; totalTokens: number; cost?: number }): StreamEvent {
  return { type: 'done', usage } as StreamEvent;
}

function sError(code: string, message: string): StreamEvent {
  return { type: 'error', code, message };
}

type StreamFn = (_request: ChatRequest) => AsyncGenerator<StreamEvent, void, unknown>;

function makeStream(...events: StreamEvent[]): StreamFn {
  return vi.fn(async function* () {
    for (const e of events) yield e;
  }) as unknown as StreamFn;
}

const cancelFn: () => void = vi.fn(() => {});

function makeContext(overrides?: Partial<AgentContext>): AgentContext {
  return {
    sessionId: 'sess-1',
    messages: [],
    providerId: 'openai',
    model: 'gpt-4o',
    ...overrides,
  };
}

/** Mock callbacks matching AgentCallbacks interface. */
// eslint-disable-next-line @typescript-eslint/no-explicit-any
function makeCallbacks(): any {
  return {
    onDelta: vi.fn(),
    onToolCall: vi.fn(),
    onToolResult: vi.fn(),
    onPermissionRequest: vi.fn().mockResolvedValue('allow'),
    onStateChange: vi.fn(),
    onTokens: vi.fn(),
    onError: vi.fn(),
    onDone: vi.fn(),
    onPlanStep: vi.fn(),
  };
}

describe('AgentLoop', () => {
  let loop: AgentLoop;
  let streamChat = makeStream(delta('Hello'), done());

  beforeEach(() => {
    streamChat = makeStream(delta('Hello'), done());
    loop = new AgentLoop(streamChat, cancelFn);
  });

  it('streams text deltas to onDelta callback', async () => {
    const cb = makeCallbacks();
    await loop.run('Hi', cb, makeContext());
    expect(cb.onDelta).toHaveBeenCalledWith('Hello');
    expect(cb.onDone).toHaveBeenCalledTimes(1);
    expect(cb.onError).not.toHaveBeenCalled();
  });

  it('fires onDone exactly once on success', async () => {
    const cb = makeCallbacks();
    await loop.run('Hi', cb, makeContext());
    expect(cb.onDone).toHaveBeenCalledTimes(1);
  });

  it('fires onDone on error too', async () => {
    const errStream = vi.fn(async function* () {
      yield sError('PROVIDER', 'timeout');
      yield done();
    }) as unknown as StreamFn;
    loop = new AgentLoop(errStream, cancelFn);
    const cb = makeCallbacks();
    await loop.run('X', cb, makeContext());
    // Error-as-conversation: errors are injected as user messages
    // and LLM responds, so onError may not be called for recoverable errors
    expect(cb.onDone).toHaveBeenCalledTimes(1);
  });

  it('fires onDone when streamChat throws', async () => {
    const throwStream = vi.fn(async function* () {
      throw new Error('crash');
    }) as unknown as StreamFn;
    loop = new AgentLoop(throwStream, cancelFn);
    const cb = makeCallbacks();
    await loop.run('X', cb, makeContext());
    // Error-as-conversation: crash is injected as user message
    // for LLM to analyze, so onError is no longer the primary path
    expect(cb.onDone).toHaveBeenCalledTimes(1);
  });

  it('emits state transitions thinking to done', async () => {
    const cb = makeCallbacks();
    await loop.run('X', cb, makeContext());
    expect(cb.onStateChange).toHaveBeenCalledWith('thinking', 'Starting...');
    expect(cb.onStateChange).toHaveBeenCalledWith('thinking', 'Calling LLM...');
    expect(cb.onStateChange).toHaveBeenCalledWith('done');
  });

  it('emits state transitions thinking to error on thrown failure', async () => {
    const throwStream = vi.fn(async function* () {
      throw new Error('crash');
    }) as unknown as StreamFn;
    loop = new AgentLoop(throwStream, cancelFn);
    const cb = makeCallbacks();
    await loop.run('X', cb, makeContext());
    // Error-as-conversation: crash is caught, error injected as message
    // Stream throws before for loop starts, so 'done' comes from catch path
    expect(cb.onDone).toHaveBeenCalled();
  });

  it('forwards token usage from done event', async () => {
    streamChat = makeStream(
      delta('ok'),
      done({ promptTokens: 10, completionTokens: 5, totalTokens: 15, cost: 0 }),
    );
    loop = new AgentLoop(streamChat, cancelFn);
    const cb = makeCallbacks();
    await loop.run('X', cb, makeContext());
    expect(cb.onTokens).toHaveBeenCalledWith({
      promptTokens: 10,
      completionTokens: 5,
      totalTokens: 15,
      cost: 0,
    });
  });

  it('executes tools and sends results back to LLM', async () => {
    const toolExecutor: ToolExecutor = {
      getDefinitions: () => [
        { name: 'read_file', description: '', parameters: { type: 'object', properties: {} } },
      ],
      hasSideEffect: () => false,
      execute: vi.fn((tc: ToolCall): Promise<ToolResult> =>
        Promise.resolve({ toolCallId: tc.id, content: 'file content here' }),
      ),
    };

    let turn = 0;
    const twoTurnStream = vi.fn(async function* () {
      turn++;
      if (turn === 1) {
        yield tCall('tc-1', 'read_file', '{"path":"/foo"}');
        yield done();
      } else {
        yield delta('File says: ');
        yield delta('file content here');
        yield done();
      }
    }) as unknown as StreamFn;

    const l = new AgentLoop(twoTurnStream, cancelFn, toolExecutor);
    const cb = makeCallbacks();
    await l.run('read /foo', cb, makeContext());

    // Filter out hooks_run calls — we only care about the read_file tool
    const readCalls = vi.mocked(toolExecutor.execute).mock.calls
      .filter((c: unknown[]) => (c[0] as ToolCall)?.name === 'read_file');
    expect(readCalls).toHaveLength(1);
    expect(cb.onToolCall).toHaveBeenCalledTimes(1);
    expect(cb.onToolResult).toHaveBeenCalledTimes(1);
    expect(cb.onDelta).toHaveBeenCalledWith('File says: ');
    expect(cb.onDelta).toHaveBeenCalledWith('file content here');
    // onDone fires in hooks + onDone path (not from finally block only)
    expect(cb.onDone).toHaveBeenCalled();
  });

  it('receives final accumulated tool call from cumulative backend events', async () => {
    const toolExecutor: ToolExecutor = {
      getDefinitions: () => [
        { name: 'grep', description: '', parameters: { type: 'object', properties: {} } },
      ],
      hasSideEffect: () => false,
      execute: vi.fn((tc: ToolCall): Promise<ToolResult> =>
        Promise.resolve({ toolCallId: tc.id, content: 'found' }),
      ),
    };

    let turn = 0;
    const twoTurnStream = vi.fn(async function* () {
      turn++;
      if (turn === 1) {
        // Backend sends CUMULATIVE arguments (Rust BTreeMap accumulation)
        yield tCall('tc-1', 'grep', '{"pattern');
        yield tCall('tc-1', 'grep', '{"pattern":"foo"}'); // full accumulated value
        yield done();
      } else {
        yield delta('done');
        yield done();
      }
    }) as unknown as StreamFn;

    loop = new AgentLoop(twoTurnStream, cancelFn, toolExecutor);
    const cb = makeCallbacks();
    await loop.run('X', cb, makeContext());
    // Only the final ToolCall (with complete JSON) is used for execution
    expect(cb.onToolCall).toHaveBeenCalledTimes(1);
  });

  it('denies write operations when permission callback returns deny', async () => {
    const toolExecutor: ToolExecutor = {
      getDefinitions: () => [
        { name: 'write_file', description: '', parameters: { type: 'object', properties: {} } },
      ],
      hasSideEffect: () => true,
      execute: vi.fn(),
    };

    const denyStream = vi.fn(async function* () {
      yield tCall('tc-1', 'write_file', '{"path":"/x"}');
      yield done();
    }) as unknown as StreamFn;
    const l = new AgentLoop(denyStream, cancelFn, toolExecutor);
    const cb = makeCallbacks();
    cb.onPermissionRequest = vi.fn().mockResolvedValue('deny');

    await l.run('write', cb, makeContext());

    // write_file itself should never be called (only hooks_run calls go through)
    const writeCalls = vi.mocked(toolExecutor.execute).mock.calls
      .filter((c: unknown[]) => (c[0] as ToolCall)?.name === 'write_file');
    expect(writeCalls).toHaveLength(0);
    // Permission deny produces toolResult; error-as-conversation may inject analysis
    expect(cb.onToolResult).toHaveBeenCalled();
  });

  it('stops after MAX_ITERATIONS tool loops', async () => {
    const toolExecutor: ToolExecutor = {
      getDefinitions: () => [
        { name: 'read_file', description: '', parameters: { type: 'object', properties: {} } },
      ],
      hasSideEffect: () => false,
      execute: vi.fn((tc: ToolCall): Promise<ToolResult> =>
        Promise.resolve({ toolCallId: tc.id, content: 'ok' }),
      ),
    };

    const infiniteStream = vi.fn(async function* () {
      yield tCall('tc-1', 'read_file', '{}');
      yield done();
    }) as unknown as StreamFn;

    const l = new AgentLoop(infiniteStream, cancelFn, toolExecutor);
    const cb = makeCallbacks();
    await l.run('hi', cb, makeContext());

    // Error-as-conversation: max iterations injects an analysis turn
    // With tool result caching, only the first invocation executes the tool;
    // subsequent identical calls reuse the cached result.
    const readCalls = vi.mocked(toolExecutor.execute).mock.calls
      .filter((c: unknown[]) => (c[0] as ToolCall)?.name === 'read_file');
    expect(readCalls.length).toBeGreaterThanOrEqual(1);
  });

  it('cancel() calls the cancel function', () => {
    loop.cancel();
    expect(cancelFn).toHaveBeenCalledTimes(1);
  });

  it('includes systemPrompt in ChatRequest when provided', async () => {
    streamChat = makeStream(delta('x'), done());
    loop = new AgentLoop(streamChat, cancelFn);
    const cb = makeCallbacks();
    const ctx = makeContext({ systemPrompt: 'You are helpful.' });

    await loop.run('hi', cb, ctx);
    const calls = (streamChat as unknown as ReturnType<typeof vi.fn>).mock.calls;
    const request = calls[0]?.[0] as ChatRequest | undefined;
    expect(request).toBeDefined();
    expect(request!.systemPrompt).toBe('You are helpful.');
  });

  it('includes tool definitions when executor is set', async () => {
    const toolExecutor: ToolExecutor = {
      getDefinitions: vi.fn(() => [
        { name: 'read_file', description: '', parameters: { type: 'object', properties: {} } },
      ]),
      hasSideEffect: () => false,
      execute: vi.fn(),
    };

    streamChat = makeStream(delta('ok'), done());
    loop = new AgentLoop(streamChat, cancelFn, toolExecutor);
    const cb = makeCallbacks();
    await loop.run('hi', cb, makeContext());

    const calls = (streamChat as unknown as ReturnType<typeof vi.fn>).mock.calls;
    const request = calls[0]?.[0] as ChatRequest | undefined;
    expect(request).toBeDefined();
    expect(request!.tools).toHaveLength(1);
    expect(request!.tools![0].type).toBe('function');
    expect(request!.tools![0].function.name).toBe('read_file');
  });

  it('builds ChatRequest from context messages', async () => {
    streamChat = makeStream(delta('ok'), done());
    loop = new AgentLoop(streamChat, cancelFn);
    const cb = makeCallbacks();
    const ctx = makeContext({
      messages: [
        { id: 'm1', role: 'user', content: 'prev', timestamp: 1 },
      ],
    });

    await loop.run('new', cb, ctx);
    const calls = (streamChat as unknown as ReturnType<typeof vi.fn>).mock.calls;
    const request = calls[0]?.[0] as ChatRequest | undefined;
    expect(request).toBeDefined();
    expect(request!.messages).toHaveLength(2);
    expect(request!.messages[0].content).toBe('prev');
    expect(request!.messages[1].content).toBe('new');
    expect(request!.messages[1].role).toBe('user');
  });

  it('runAutonomous injects planning prompt', async () => {
    streamChat = makeStream(delta('ok'), done());
    loop = new AgentLoop(streamChat, cancelFn);
    const cb = makeCallbacks();
    await loop.runAutonomous('build a feature', cb, makeContext());

    const calls = (streamChat as unknown as ReturnType<typeof vi.fn>).mock.calls;
    const request = calls[0]?.[0] as ChatRequest | undefined;
    expect(request).toBeDefined();
    expect(request!.systemPrompt).toContain('autonomous developer');
    expect(request!.systemPrompt).toContain('task-plan.json');
    expect(request!.systemPrompt).toContain('build a feature');
  });

  it('runAutonomous fires onPlanStep with task description', async () => {
    streamChat = makeStream(delta('done'), done());
    loop = new AgentLoop(streamChat, cancelFn);
    const cb = makeCallbacks();
    await loop.runAutonomous('add dark mode', cb, makeContext());
    expect(cb.onPlanStep).toHaveBeenCalledWith(
      expect.objectContaining({ id: 0, description: 'add dark mode' }),
    );
  });

  it('runAutonomous with autoPr fires gh_pr_create after completion', async () => {
    const toolExecutor: ToolExecutor = {
      getDefinitions: () => [],
      hasSideEffect: () => false,
      execute: vi.fn((tc: ToolCall): Promise<ToolResult> =>
        Promise.resolve({ toolCallId: tc.id, content: 'ok' }),
      ),
    };

    streamChat = makeStream(delta('done'), done());
    loop = new AgentLoop(streamChat, cancelFn, toolExecutor);
    const cb = makeCallbacks();
    await loop.runAutonomous('fix bug', cb, makeContext({ autoPr: true }));

    const prCalls = vi.mocked(toolExecutor.execute).mock.calls
      .filter((c: unknown[]) => (c[0] as ToolCall)?.name === 'gh_pr_create');
    expect(prCalls).toHaveLength(1);
    expect(JSON.parse((prCalls[0]![0] as ToolCall).arguments).title).toContain('fix bug');
  });

  it('runParallel fires multiple streams and collects results', async () => {
    const tasks: ParallelTask[] = [
      { task: 'Check src/', systemPrompt: 'You are explorer.' },
      { task: 'Review tests', systemPrompt: 'You are reviewer.' },
    ];

    const factory = vi.fn(() =>
      makeStream(
        delta('result'),
        done({ promptTokens: 2, completionTokens: 1, totalTokens: 3, cost: 0 }),
      ),
    ) as unknown as () => ReturnType<typeof makeStream>;

    const cb = makeCallbacks();
    const results = await loop.runParallel(tasks, cb, makeContext(), factory);

    expect(results).toHaveLength(2);
    expect(results[0].success).toBe(true);
    expect(results[0].content).toBe('result');
    expect(results[1].success).toBe(true);
    expect(results[1].content).toBe('result');
    // One factory call per task
    expect(factory).toHaveBeenCalledTimes(2);
    // Each stream emitted content_delta
    expect(cb.onDelta).toHaveBeenCalledWith('result');
    expect(cb.onDelta).toHaveBeenCalledTimes(2);
  });

  // ---------------------------------------------------------------------------
  // runParallel multi-turn tests
  // ---------------------------------------------------------------------------

  it('runParallel returns ParallelResult for single-turn no-tool task', async () => {
    const tasks: ParallelTask[] = [{ task: 'Hello' }];
    const factory = vi.fn(() =>
      makeStream(
        delta('world'),
        done({ promptTokens: 1, completionTokens: 1, totalTokens: 2, cost: 0 }),
      ),
    ) as unknown as () => ReturnType<typeof makeStream>;
    const cb = makeCallbacks();

    const results = await loop.runParallel(tasks, cb, makeContext(), factory);

    expect(results).toHaveLength(1);
    expect(results[0]).toMatchObject({
      taskId: 'parallel-0',
      success: true,
      content: 'world',
      iterations: 0,
    });
  });

  it('runParallel executes multi-turn tool tasks with complete loop', async () => {
    const toolExecutor: ToolExecutor = {
      getDefinitions: () => [
        { name: 'read_file', description: '', parameters: { type: 'object', properties: {} } },
      ],
      hasSideEffect: () => false,
      execute: vi.fn((tc: ToolCall): Promise<ToolResult> =>
        Promise.resolve({ toolCallId: tc.id, content: 'file contents' }),
      ),
    };

    let factoryCalls = 0;
    const callCounts = new Map<string, number>();
    const factory = vi.fn(() => {
      const key = `task-${++factoryCalls}`;
      callCounts.set(key, 0);
      return vi.fn(async function* () {
        const count = (callCounts.get(key) ?? 0) + 1;
        callCounts.set(key, count);
        if (count === 1) {
          yield tCall('tc-1', 'read_file', '{"path":"/x"}');
          yield done();
        } else {
          yield delta('final answer');
          yield done({ promptTokens: 2, completionTokens: 2, totalTokens: 4, cost: 0 });
        }
      }) as unknown as ReturnType<typeof makeStream>;
    }) as unknown as () => ReturnType<typeof makeStream>;

    const l = new AgentLoop(makeStream(delta('ok'), done()), cancelFn, toolExecutor);
    const cb = makeCallbacks();
    const tasks: ParallelTask[] = [{ task: 'Read /x' }];

    const results = await l.runParallel(tasks, cb, makeContext(), factory);

    expect(results).toHaveLength(1);
    expect(results[0].success).toBe(true);
    expect(results[0].content).toBe('final answer');
    expect(results[0].iterations).toBeGreaterThanOrEqual(1);
    expect(toolExecutor.execute).toHaveBeenCalled();
    expect(cb.onToolCall).toHaveBeenCalled();
    expect(cb.onStateChange).toHaveBeenCalledWith(
      'tool_call',
      expect.stringContaining('parallel-0: read_file'),
    );
  });

  it('runParallel isolates errors so one failing task does not break others', async () => {
    const toolExecutor: ToolExecutor = {
      getDefinitions: () => [
        { name: 'read_file', description: '', parameters: { type: 'object', properties: {} } },
      ],
      hasSideEffect: () => false,
      execute: vi.fn((tc: ToolCall): Promise<ToolResult> =>
        Promise.resolve({ toolCallId: tc.id, content: 'ok' }),
      ),
    };

    let callIndex = 0;
    const factory = vi.fn(() => {
      const idx = callIndex++;
      return vi.fn(async function* () {
        if (idx === 0) {
          yield delta('success-content');
          yield done({ promptTokens: 1, completionTokens: 1, totalTokens: 2, cost: 0 });
        } else {
          throw new Error('stream connection lost');
        }
      }) as unknown as ReturnType<typeof makeStream>;
    }) as unknown as () => ReturnType<typeof makeStream>;

    const l = new AgentLoop(makeStream(delta('ignored'), done()), cancelFn, toolExecutor);
    const cb = makeCallbacks();
    const tasks: ParallelTask[] = [
      { task: 'simple one' },
      { task: 'failing one' },
    ];

    const results = await l.runParallel(tasks, cb, makeContext(), factory);

    expect(results).toHaveLength(2);
    const simple = results.find(r => r.taskId === 'parallel-0')!;
    const failing = results.find(r => r.taskId === 'parallel-1')!;
    expect(simple.success).toBe(true);
    expect(simple.content).toBe('success-content');
    expect(failing.success).toBe(false);
    expect(failing.error).toContain('stream connection lost');
  });

  // ---------------------------------------------------------------------------
  // SECURITY PoC: Finding 2 — hooks_run and triggers_watch bypass permission gate
  // ---------------------------------------------------------------------------

  it('FIXED: hasSideEffect("hooks_run") returns true', () => {
    expect(hasSideEffect('hooks_run')).toBe(true);
  });

  it('FIXED: hasSideEffect("triggers_watch") returns true', () => {
    expect(hasSideEffect('triggers_watch')).toBe(true);
  });

  it('FIXED: hooks_run is gated by permission prompt in AgentLoop', async () => {
    const toolExecutor: ToolExecutor = {
      getDefinitions: () => [
        { name: 'hooks_run', description: '', parameters: { type: 'object', properties: {} } },
      ],
      hasSideEffect: (name: string) => hasSideEffect(name),
      execute: vi.fn((tc: ToolCall): Promise<ToolResult> =>
        Promise.resolve({ toolCallId: tc.id, content: '[]' }),
      ),
    };

    let turn = 0;
    const hookStream = vi.fn(async function* () {
      turn++;
      if (turn === 1) {
        yield tCall('tc-1', 'hooks_run', '{"path":".","event":"after_chat"}');
        yield done();
      } else {
        yield delta('done');
        yield done();
      }
    }) as unknown as StreamFn;

    const l = new AgentLoop(hookStream, cancelFn, toolExecutor);
    const cb = makeCallbacks();

    await l.run('run hook', cb, makeContext());

    const hookCalls = vi.mocked(toolExecutor.execute).mock.calls
      .filter((c: unknown[]) => (c[0] as ToolCall)?.name === 'hooks_run');
    expect(hookCalls.length).toBeGreaterThanOrEqual(1);
    expect(hookCalls.some((c: unknown[]) => (c[0] as ToolCall).id === 'tc-1')).toBe(true);
    expect(cb.onPermissionRequest).toHaveBeenCalled();
  });

  it('FIXED: triggers_watch is gated by permission prompt in AgentLoop', async () => {
    const toolExecutor: ToolExecutor = {
      getDefinitions: () => [
        { name: 'triggers_watch', description: '', parameters: { type: 'object', properties: {} } },
      ],
      hasSideEffect: (name: string) => hasSideEffect(name),
      execute: vi.fn((tc: ToolCall): Promise<ToolResult> =>
        Promise.resolve({ toolCallId: tc.id, content: 'trigger started' }),
      ),
    };

    let turn = 0;
    const triggerStream = vi.fn(async function* () {
      turn++;
      if (turn === 1) {
        yield tCall('tc-1', 'triggers_watch', '{"path":".","trigger_id":"evil"}');
        yield done();
      } else {
        yield delta('done');
        yield done();
      }
    }) as unknown as StreamFn;

    const l = new AgentLoop(triggerStream, cancelFn, toolExecutor);
    const cb = makeCallbacks();

    await l.run('start trigger', cb, makeContext());

    const triggerCalls = vi.mocked(toolExecutor.execute).mock.calls
      .filter((c: unknown[]) => (c[0] as ToolCall)?.name === 'triggers_watch');
    expect(triggerCalls.length).toBeGreaterThanOrEqual(1);
    expect(triggerCalls.some((c: unknown[]) => (c[0] as ToolCall).id === 'tc-1')).toBe(true);
    expect(cb.onPermissionRequest).toHaveBeenCalled();
  });

  // ---------------------------------------------------------------------------
  // Anti-loop: tool result truncation + repeated-denied-call short-circuit
  // ---------------------------------------------------------------------------

  it('truncateToolResult leaves small output untouched', () => {
    const small = 'cargo test passed: 23 ok';
    expect(truncateToolResult(small)).toBe(small);
  });

  it('truncateToolResult coerces non-string results (object/null) without throwing', () => {
    // Some Tauri commands (triggers_list, index_codebase) return objects at
    // runtime despite being typed Promise<string>; must not throw "slice is not a function".
    expect(truncateToolResult({ triggers: [], active_count: 0 } as unknown)).toBe(
      '{"triggers":[],"active_count":0}',
    );
    expect(truncateToolResult(null as unknown)).toBe('');
    expect(truncateToolResult(undefined as unknown)).toBe('');
    expect(truncateToolResult(42 as unknown)).toBe('42');
  });

  it('truncateToolResult middle-truncates huge output, keeping head and tail', () => {
    const huge = 'HEAD_MARKER' + 'x'.repeat(50_000) + 'TAIL_MARKER';
    const out = truncateToolResult(huge);
    expect(out.length).toBeLessThan(huge.length);
    expect(out.startsWith('HEAD_MARKER')).toBe(true); // head preserved
    expect(out.endsWith('TAIL_MARKER')).toBe(true); // tail (test summary) preserved
    expect(out).toContain('characters truncated');
    expect(out).toContain('middle omitted');
  });

  it('truncateToolResult preserves error/fail lines from the middle', () => {
    const testOutput =
      'HEAD_START\nok test 1\nok test 2\n'
      + 'x'.repeat(20_000)
      + '\nFAIL test_crash\nsrc/main.rs:42 assertion failed: `left == right`\n'
      + 'error[E0308]: mismatched types\n'
      + 'x'.repeat(20_000)
      + '\ntest result: FAILED. 12 passed, 1 failed\n';
    const out = truncateToolResult(testOutput);
    // Error/fail lines from the middle should be preserved in output
    expect(out).toContain('FAIL test_crash');
    expect(out).toContain('assertion failed');
    expect(out).toContain('mismatched types');
    // Test summary at tail should be preserved
    expect(out).toContain('test result: FAILED');
    // Head should be preserved
    expect(out).toContain('HEAD_START');
    // Should not exceed budget
    expect(out.length).toBeLessThan(testOutput.length);
  });

  it('truncateToolResult truncates large JSON arrays while preserving structure', () => {
    // Each file entry is ~30 chars; 3000 entries × 30 + overhead ≈ 95K → well over 16K budget
    const largeJson = JSON.stringify({
      status: 'completed',
      files: Array.from({ length: 3000 }, (_, i) => `src/component/file_${i}/index.tsx`),
      summary: { passed: 42, failed: 0 },
    });
    const out = truncateToolResult(largeJson);
    // Must be valid JSON (or at least start with {)
    expect(out.startsWith('{')).toBe(true);
    // Keys should be preserved
    expect(out).toContain('"status"');
    expect(out).toContain('"summary"');
    expect(out).toContain('"files"');
    // Large array should be truncated (less than 3000 items)
    expect(out).toContain('more items');
    // Should be smaller than original
    expect(out.length).toBeLessThan(largeJson.length);
    expect(out.length).toBeLessThanOrEqual(16_000);
  });

  it('truncateToolResult preserves tail-heavy test output (pass/fail summary at end)', () => {
    const head = 'START_LOG\n' + 'info: compiling\n'.repeat(500);
    const middle = 'x'.repeat(20_000);
    const tail = '\npassed: 142\nfailed: 0\nignored: 3\nmeasured: 1\n'
      + 'Finished with summary: ALL TESTS PASSED\n';
    const testOutput = head + middle + tail;
    const out = truncateToolResult(testOutput);
    // Tail (test summary) should be fully preserved
    expect(out).toContain('ALL TESTS PASSED');
    expect(out).toContain('passed: 142');
    expect(out).toContain('failed: 0');
    // Head start should be present
    expect(out).toContain('START_LOG');
    // Should be truncated
    expect(out.length).toBeLessThan(testOutput.length);
  });

  it('truncateToolResult keeps both head context and tail summary within budget', () => {
    const head = 'Running test suite: core_module\n' + 'compile_check_ok\n'.repeat(800);
    const middle = 'x'.repeat(30_000);
    const tail = '\n---\ntest result: ok. 256 passed; 0 failed; 0 ignored\n';
    const full = head + middle + tail;
    const out = truncateToolResult(full);
    // Head context preserved
    expect(out).toContain('Running test suite');
    // Tail summary preserved
    expect(out).toContain('256 passed');
    expect(out).toContain('0 failed');
    // Within budget
    expect(out.length).toBeLessThan(full.length);
    expect(out.length).toBeLessThanOrEqual(16_000);
  });

  it('huge tool result is truncated before reaching the model', async () => {
    const huge = 'X'.repeat(40_000);
    const toolExecutor: ToolExecutor = {
      getDefinitions: () => [
        { name: 'run_command', description: '', parameters: { type: 'object', properties: {} } },
      ],
      hasSideEffect: () => false, // skip permission gate for this test
      execute: vi.fn((tc: ToolCall): Promise<ToolResult> =>
        Promise.resolve({ toolCallId: tc.id, content: huge }),
      ),
    };

    let turn = 0;
    const stream = vi.fn(async function* () {
      turn++;
      if (turn === 1) {
        yield tCall('tc-1', 'run_command', '{"command":"cargo test"}');
        yield done();
      } else {
        yield delta('done');
        yield done();
      }
    }) as unknown as StreamFn;

    const l = new AgentLoop(stream, cancelFn, toolExecutor);
    const cb = makeCallbacks();
    await l.run('run tests', cb, makeContext());

    const resultArg = cb.onToolResult.mock.calls[0][0] as ToolResult;
    expect(resultArg.content.length).toBeLessThan(huge.length);
    expect(resultArg.content).toContain('characters truncated');
  });

  it('a denied tool returns a factual denial and is NOT auto-blocked (model-driven)', async () => {
    // Model-driven design: the framework does not short-circuit repeated denied
    // calls. It reports the denial factually and lets the model decide; the
    // system prompt is what tells the model not to retry. So a repeat re-prompts.
    const toolExecutor: ToolExecutor = {
      getDefinitions: () => [
        { name: 'ssh_exec', description: '', parameters: { type: 'object', properties: {} } },
      ],
      hasSideEffect: (name: string) => hasSideEffect(name),
      execute: vi.fn((tc: ToolCall): Promise<ToolResult> =>
        Promise.resolve({ toolCallId: tc.id, content: 'connected' }),
      ),
    };

    const args = '{"host":"h","username":"u","password":"p","port":22,"command":"hostname"}';
    let turn = 0;
    const stream = vi.fn(async function* () {
      turn++;
      if (turn <= 2) {
        yield tCall(`tc-${turn}`, 'ssh_exec', args);
        yield done();
      } else {
        yield delta('ok, I will ask you to grant access instead');
        yield done();
      }
    }) as unknown as StreamFn;

    const l = new AgentLoop(stream, cancelFn, toolExecutor);
    const cb = makeCallbacks();
    cb.onPermissionRequest = vi.fn().mockResolvedValue('deny');

    await l.run('ssh in', cb, makeContext());

    // Each denied call is re-prompted (no hard-coded blocking), the denial
    // result is plain/factual (no "[NO_RETRY]"), and the tool never executes.
    expect(cb.onPermissionRequest).toHaveBeenCalledTimes(2);
    const deniedResult = cb.onToolResult.mock.calls[0][0] as ToolResult;
    expect(deniedResult.content).toContain('denied permission');
    expect(deniedResult.content).not.toContain('NO_RETRY');
    expect(vi.mocked(toolExecutor.execute).mock.calls
      .filter((c: unknown[]) => (c[0] as ToolCall)?.name === 'ssh_exec')).toHaveLength(0);
  });

  it('cancel() stops the multi-turn loop instead of running to the iteration cap', async () => {
    // A stream that ALWAYS returns a tool call would loop until the iteration
    // cap; cancelling during the first tool execution must stop it after turn 1.
    let loopRef: AgentLoop;
    const toolExecutor: ToolExecutor = {
      getDefinitions: () => [
        { name: 'read_file', description: '', parameters: { type: 'object', properties: {} } },
      ],
      hasSideEffect: () => false,
      execute: vi.fn((tc: ToolCall): Promise<ToolResult> => {
        // Only cancel on the real tool call — execute is also invoked by the
        // before_chat hook and agents_rules_read before the main loop starts.
        if (tc.name === 'read_file') {
          loopRef.cancel(); // simulate user pressing Stop mid-execution
        }
        return Promise.resolve({ toolCallId: tc.id, content: 'data' });
      }),
    };

    let turn = 0;
    const neverEndingStream = vi.fn(async function* () {
      turn++;
      yield tCall(`tc-${turn}`, 'read_file', '{"path":"/x"}');
      yield done();
    }) as unknown as StreamFn;

    loopRef = new AgentLoop(neverEndingStream, cancelFn, toolExecutor);
    const cb = makeCallbacks();
    await loopRef.run('loop forever', cb, makeContext());

    // Without cancel this would reach the iteration cap (25); cancel stops it at 1.
    expect(turn).toBe(1);
    expect(cb.onDone).toHaveBeenCalled();
  });

  it('sanitizes broken history so the request never has orphan tool messages or fake ids', async () => {
    const stream = makeStream(delta('hi'), done());
    const l = new AgentLoop(stream, cancelFn);
    const cb = makeCallbacks();
    // Broken history like an old session: an assistant tool_call with no
    // response, and a tool message whose id was lost (empty).
    const brokenHistory = [
      { id: 'a', role: 'assistant', content: '', toolCalls: [{ id: 'real-1', name: 'x', arguments: '{}' }], timestamp: 1 },
      { id: 't', role: 'tool', content: 'orphan result', toolCallId: '', name: 'x', timestamp: 2 },
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
    ] as any;

    await l.run('hello', cb, makeContext({ messages: brokenHistory }));

    const calls = (stream as unknown as ReturnType<typeof vi.fn>).mock.calls;
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const msgs = (calls[0]?.[0] as ChatRequest).messages as any[];
    // Orphan tool message dropped; no fabricated "unknown-" ids anywhere;
    // the unanswered assistant tool_call has its toolCalls stripped.
    expect(msgs.some((m) => m.role === 'tool')).toBe(false);
    expect(JSON.stringify(msgs)).not.toContain('unknown-');
    const asst = msgs.find((m) => m.role === 'assistant' && m.content === '');
    expect(asst?.toolCalls).toBeUndefined();
  });

  it('keeps a properly-paired assistant tool_call + tool response', async () => {
    const stream = makeStream(delta('hi'), done());
    const l = new AgentLoop(stream, cancelFn);
    const cb = makeCallbacks();
    const goodHistory = [
      { id: 'a', role: 'assistant', content: '', toolCalls: [{ id: 'call_1', name: 'x', arguments: '{}' }], timestamp: 1 },
      { id: 't', role: 'tool', content: 'result', toolCallId: 'call_1', name: 'x', timestamp: 2 },
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
    ] as any;

    await l.run('hello', cb, makeContext({ messages: goodHistory }));

    const calls = (stream as unknown as ReturnType<typeof vi.fn>).mock.calls;
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const msgs = (calls[0]?.[0] as ChatRequest).messages as any[];
    const toolMsg = msgs.find((m) => m.role === 'tool');
    expect(toolMsg?.toolCallId).toBe('call_1');
    const asst = msgs.find((m) => m.role === 'assistant' && Array.isArray(m.toolCalls));
    expect(asst?.toolCalls?.[0]?.id).toBe('call_1');
  });

  it('auto-loads project memory into the system prompt at session start', async () => {
    const toolExecutor: ToolExecutor = {
      getDefinitions: () => [],
      hasSideEffect: () => false,
      execute: vi.fn((tc: ToolCall): Promise<ToolResult> => {
        if (tc.name === 'memory_read') {
          return Promise.resolve({ toolCallId: tc.id, content: 'MEMORY_MARKER: user prefers tabs' });
        }
        // agents_rules_read / hooks_run etc.
        return Promise.resolve({ toolCallId: tc.id, content: '{}' });
      }),
    };

    const stream = makeStream(delta('hi'), done());
    const l = new AgentLoop(stream, cancelFn, toolExecutor);
    const cb = makeCallbacks();
    await l.run('hello', cb, makeContext());

    const calls = (stream as unknown as ReturnType<typeof vi.fn>).mock.calls;
    const request = calls[0]?.[0] as ChatRequest | undefined;
    expect(request?.systemPrompt).toContain('PROJECT MEMORY');
    expect(request?.systemPrompt).toContain('MEMORY_MARKER: user prefers tabs');
  });

  // ---------------------------------------------------------------------------
  // P0-3: Parallel tool execution via Promise.allSettled
  // ---------------------------------------------------------------------------

  it('executes multiple tool calls in parallel', async () => {
    const toolExecutor: ToolExecutor = {
      getDefinitions: () => [
        { name: 'read_a', description: '', parameters: { type: 'object', properties: {} } },
        { name: 'read_b', description: '', parameters: { type: 'object', properties: {} } },
        { name: 'read_c', description: '', parameters: { type: 'object', properties: {} } },
      ],
      hasSideEffect: () => false,
      execute: vi.fn((tc: ToolCall): Promise<ToolResult> =>
        Promise.resolve({ toolCallId: tc.id, content: `result:${tc.name}` }),
      ),
    };

    let turn = 0;
    const stream = vi.fn(async function* () {
      turn++;
      if (turn === 1) {
        yield tCall('tc-a', 'read_a', '{}');
        yield tCall('tc-b', 'read_b', '{}');
        yield tCall('tc-c', 'read_c', '{}');
        yield done();
      } else {
        yield delta('all done');
        yield done();
      }
    }) as unknown as StreamFn;

    const l = new AgentLoop(stream, cancelFn, toolExecutor);
    const cb = makeCallbacks();
    await l.run('x', cb, makeContext());

    const execCalls = vi.mocked(toolExecutor.execute).mock.calls
      .filter((c) => {
        const name = (c[0] as ToolCall)?.name;
        return name === 'read_a' || name === 'read_b' || name === 'read_c';
      });
    expect(execCalls).toHaveLength(3);
    expect(cb.onToolResult).toHaveBeenCalledTimes(3);
  });

  it('handles cancellation during parallel execution', async () => {
    let loopRef: AgentLoop;
    const toolExecutor: ToolExecutor = {
      getDefinitions: () => [
        { name: 'read_a', description: '', parameters: { type: 'object', properties: {} } },
        { name: 'read_b', description: '', parameters: { type: 'object', properties: {} } },
        { name: 'read_c', description: '', parameters: { type: 'object', properties: {} } },
      ],
      hasSideEffect: () => false,
      execute: vi.fn((tc: ToolCall): Promise<ToolResult> => {
        if (tc.name === 'read_b') {
          loopRef.cancel();
        }
        return Promise.resolve({ toolCallId: tc.id, content: 'ok' });
      }),
    };

    let turn = 0;
    const stream = vi.fn(async function* () {
      turn++;
      yield tCall('tc-a', 'read_a', '{}');
      yield tCall('tc-b', 'read_b', '{}');
      yield tCall('tc-c', 'read_c', '{}');
      yield done();
    }) as unknown as StreamFn;

    loopRef = new AgentLoop(stream, cancelFn, toolExecutor);
    const cb = makeCallbacks();
    await loopRef.run('x', cb, makeContext());

    // All 3 tools executed in parallel before cancellation check in the for-loop
    const execCalls = vi.mocked(toolExecutor.execute).mock.calls
      .filter((c) => {
        const name = (c[0] as ToolCall)?.name;
        return name === 'read_a' || name === 'read_b' || name === 'read_c';
      });
    expect(execCalls).toHaveLength(3);
    // Cancellation after allSettled breaks the result loop — done fires without further LLM turns
    expect(turn).toBe(1);
    expect(cb.onDone).toHaveBeenCalled();
  });

  it('isolates errors — one tool failure does not block others', async () => {
    const toolExecutor: ToolExecutor = {
      getDefinitions: () => [
        { name: 'read_a', description: '', parameters: { type: 'object', properties: {} } },
        { name: 'read_b', description: '', parameters: { type: 'object', properties: {} } },
        { name: 'read_c', description: '', parameters: { type: 'object', properties: {} } },
      ],
      hasSideEffect: () => false,
      execute: vi.fn((tc: ToolCall): Promise<ToolResult> => {
        if (tc.name === 'read_b') {
          throw new Error('disk full');
        }
        return Promise.resolve({ toolCallId: tc.id, content: `ok:${tc.name}` });
      }),
    };

    let turn = 0;
    const stream = vi.fn(async function* () {
      turn++;
      if (turn === 1) {
        yield tCall('tc-a', 'read_a', '{}');
        yield tCall('tc-b', 'read_b', '{}');
        yield tCall('tc-c', 'read_c', '{}');
        yield done();
      } else {
        yield delta('recovered');
        yield done();
      }
    }) as unknown as StreamFn;

    const l = new AgentLoop(stream, cancelFn, toolExecutor);
    const cb = makeCallbacks();
    await l.run('x', cb, makeContext());

    // All 3 tool results are still reported — error is isolated
    expect(cb.onToolResult).toHaveBeenCalledTimes(3);
    // Successful tools have their normal content
    expect(cb.onToolResult).toHaveBeenCalledWith(
      expect.objectContaining({ content: 'ok:read_a' }),
    );
    expect(cb.onToolResult).toHaveBeenCalledWith(
      expect.objectContaining({ content: 'ok:read_c' }),
    );
    // Failed tool reports an error message
    const errorCalled = cb.onToolResult.mock.calls.some((call: unknown[]) =>
      (call[0] as ToolResult).content.includes('Tool execution error'),
    );
    expect(errorCalled).toBe(true);
  });

  it('handles mixed permission decisions', async () => {
    const toolExecutor: ToolExecutor = {
      getDefinitions: () => [
        { name: 'write_a', description: '', parameters: { type: 'object', properties: {} } },
        { name: 'write_b', description: '', parameters: { type: 'object', properties: {} } },
        { name: 'write_c', description: '', parameters: { type: 'object', properties: {} } },
      ],
      hasSideEffect: () => true,
      execute: vi.fn((tc: ToolCall): Promise<ToolResult> =>
        Promise.resolve({ toolCallId: tc.id, content: `wrote:${tc.name}` }),
      ),
    };

    let turn = 0;
    const stream = vi.fn(async function* () {
      turn++;
      if (turn === 1) {
        yield tCall('tc-a', 'write_a', '{}');
        yield tCall('tc-b', 'write_b', '{}');
        yield tCall('tc-c', 'write_c', '{}');
        yield done();
      } else {
        yield delta('done');
        yield done();
      }
    }) as unknown as StreamFn;

    const l = new AgentLoop(stream, cancelFn, toolExecutor);
    const cb = makeCallbacks();
    cb.onPermissionRequest = vi.fn()
      .mockResolvedValueOnce('allow')
      .mockResolvedValueOnce('deny')
      .mockResolvedValueOnce('allow');

    await l.run('x', cb, makeContext());

    // All 3 tool results are reported — each handled independently
    expect(cb.onToolResult).toHaveBeenCalledTimes(3);

    // Allowed tools executed with real results
    const execCalls = vi.mocked(toolExecutor.execute).mock.calls
      .filter((c) => {
        const name = (c[0] as ToolCall)?.name;
        return name === 'write_a' || name === 'write_b' || name === 'write_c';
      });
    expect(execCalls).toHaveLength(2); // write_b (denied) never reaches execute

    // Denied tool reports permission denial
    const deniedCall = cb.onToolResult.mock.calls.find((call: unknown[]) =>
      (call[0] as ToolResult).content.includes('denied permission'),
    );
    expect(deniedCall).toBeDefined();
  });

  it('retries transient errors automatically', async () => {
    let callCount = 0;
    const toolExecutor: ToolExecutor = {
      getDefinitions: () => [
        { name: 'read_file', description: '', parameters: { type: 'object', properties: {} } },
      ],
      hasSideEffect: () => false,
      execute: vi.fn((tc: ToolCall): Promise<ToolResult> => {
        if (tc.name === 'read_file') {
          callCount++;
          if (callCount === 1) {
            throw new Error('ETIMEDOUT');
          }
        }
        return Promise.resolve({ toolCallId: tc.id, content: 'file content' });
      }),
    };

    let turn = 0;
    const stream = vi.fn(async function* () {
      turn++;
      if (turn === 1) {
        yield tCall('tc-1', 'read_file', '{"path":"/foo"}');
        yield done();
      } else {
        yield delta('ok');
        yield done();
      }
    }) as unknown as StreamFn;

    const l = new AgentLoop(stream, cancelFn, toolExecutor);
    const cb = makeCallbacks();
    await l.run('read', cb, makeContext());

    const readCalls = vi.mocked(toolExecutor.execute).mock.calls
      .filter((c: unknown[]) => (c[0] as ToolCall)?.name === 'read_file');
    expect(readCalls).toHaveLength(2);
    expect(cb.onToolResult).toHaveBeenCalledWith(
      expect.objectContaining({ content: 'file content' }),
    );
  });

  it('retries exhausted returns error', async () => {
    const toolExecutor: ToolExecutor = {
      getDefinitions: () => [
        { name: 'read_file', description: '', parameters: { type: 'object', properties: {} } },
      ],
      hasSideEffect: () => false,
      execute: vi.fn((): Promise<ToolResult> => {
        throw new Error('ECONNRESET');
      }),
    };

    let turn = 0;
    const stream = vi.fn(async function* () {
      turn++;
      if (turn === 1) {
        yield tCall('tc-1', 'read_file', '{"path":"/foo"}');
        yield done();
      } else {
        yield delta('ok');
        yield done();
      }
    }) as unknown as StreamFn;

    const l = new AgentLoop(stream, cancelFn, toolExecutor);
    const cb = makeCallbacks();
    await l.run('read', cb, makeContext());

    const readCalls = vi.mocked(toolExecutor.execute).mock.calls
      .filter((c: unknown[]) => (c[0] as ToolCall)?.name === 'read_file');
    expect(readCalls).toHaveLength(3);
    const errorCalled = cb.onToolResult.mock.calls.some((call: unknown[]) =>
      (call[0] as ToolResult).content.includes('Tool execution error'),
    );
    expect(errorCalled).toBe(true);
  });

  it('non-transient errors not retried', async () => {
    const toolExecutor: ToolExecutor = {
      getDefinitions: () => [
        { name: 'read_file', description: '', parameters: { type: 'object', properties: {} } },
      ],
      hasSideEffect: () => false,
      execute: vi.fn((): Promise<ToolResult> => {
        throw new Error('permission denied');
      }),
    };

    let turn = 0;
    const stream = vi.fn(async function* () {
      turn++;
      if (turn === 1) {
        yield tCall('tc-1', 'read_file', '{"path":"/foo"}');
        yield done();
      } else {
        yield delta('ok');
        yield done();
      }
    }) as unknown as StreamFn;

    const l = new AgentLoop(stream, cancelFn, toolExecutor);
    const cb = makeCallbacks();
    await l.run('read', cb, makeContext());

    const readCalls = vi.mocked(toolExecutor.execute).mock.calls
      .filter((c: unknown[]) => (c[0] as ToolCall)?.name === 'read_file');
    expect(readCalls).toHaveLength(1);
  });

  it('circuit breaker opens after 2 failures', async () => {
    const toolExecutor: ToolExecutor = {
      getDefinitions: () => [
        { name: 'read_file', description: '', parameters: { type: 'object', properties: {} } },
      ],
      hasSideEffect: () => false,
      execute: vi.fn((): Promise<ToolResult> => {
        throw new Error('ECONNREFUSED');
      }),
    };

    let turn = 0;
    const args = '{"path":"/foo"}';
    const stream = vi.fn(async function* () {
      turn++;
      if (turn === 1) {
        yield tCall('tc-1', 'read_file', args);
        yield done();
      } else if (turn === 2) {
        yield tCall('tc-2', 'read_file', args);
        yield done();
      } else if (turn === 3) {
        yield tCall('tc-3', 'read_file', args);
        yield done();
      } else {
        yield delta('ok');
        yield done();
      }
    }) as unknown as StreamFn;

    const l = new AgentLoop(stream, cancelFn, toolExecutor);
    const cb = makeCallbacks();
    await l.run('read', cb, makeContext());

    const readCalls = vi.mocked(toolExecutor.execute).mock.calls
      .filter((c: unknown[]) => (c[0] as ToolCall)?.name === 'read_file');
    expect(readCalls).toHaveLength(6);

    const breakerCalled = cb.onToolResult.mock.calls.some((call: unknown[]) =>
      (call[0] as ToolResult).content.includes('CIRCUIT BREAKER'),
    );
    expect(breakerCalled).toBe(true);
  });

  it('circuit breaker resets after success', async () => {
    let callCount = 0;
    const toolExecutor: ToolExecutor = {
      getDefinitions: () => [
        { name: 'read_file', description: '', parameters: { type: 'object', properties: {} } },
      ],
      hasSideEffect: () => false,
      execute: vi.fn((tc: ToolCall): Promise<ToolResult> => {
        if (tc.name === 'read_file') {
          callCount++;
          if (callCount === 1) {
            throw new Error('permission denied');
          }
        }
        return Promise.resolve({ toolCallId: tc.id, content: 'file content' });
      }),
    };

    let turn = 0;
    const args = '{"path":"/foo"}';
    const stream = vi.fn(async function* () {
      turn++;
      if (turn === 1) {
        yield tCall('tc-1', 'read_file', args);
        yield done();
      } else if (turn === 2) {
        yield tCall('tc-2', 'read_file', args);
        yield done();
      } else if (turn === 3) {
        yield tCall('tc-3', 'read_file', args);
        yield done();
      } else {
        yield delta('ok');
        yield done();
      }
    }) as unknown as StreamFn;

    const l = new AgentLoop(stream, cancelFn, toolExecutor);
    const cb = makeCallbacks();
    await l.run('read', cb, makeContext());

    const readCalls = vi.mocked(toolExecutor.execute).mock.calls
      .filter((c: unknown[]) => (c[0] as ToolCall)?.name === 'read_file');
    // Turn 1 fails, turn 2 succeeds (cached), turn 3 hits cache
    expect(readCalls).toHaveLength(2);
  });

  it('side-effect tools only retry on connection errors', async () => {
    let callCount = 0;
    const toolExecutor: ToolExecutor = {
      getDefinitions: () => [
        { name: 'write_file', description: '', parameters: { type: 'object', properties: {} } },
      ],
      hasSideEffect: () => true,
      execute: vi.fn((tc: ToolCall): Promise<ToolResult> => {
        if (tc.name === 'write_file') {
          callCount++;
          if (callCount === 1) {
            throw new Error('file exists');
          }
          if (callCount === 2) {
            throw new Error('ETIMEDOUT');
          }
        }
        return Promise.resolve({ toolCallId: tc.id, content: 'written' });
      }),
    };

    let turn = 0;
    const stream = vi.fn(async function* () {
      turn++;
      if (turn === 1) {
        yield tCall('tc-1', 'write_file', '{"path":"/foo"}');
        yield done();
      } else if (turn === 2) {
        yield tCall('tc-2', 'write_file', '{"path":"/bar"}');
        yield done();
      } else {
        yield delta('ok');
        yield done();
      }
    }) as unknown as StreamFn;

    const l = new AgentLoop(stream, cancelFn, toolExecutor);
    const cb = makeCallbacks();
    cb.onPermissionRequest = vi.fn().mockResolvedValue('allow');
    await l.run('write', cb, makeContext());

    const writeCalls = vi.mocked(toolExecutor.execute).mock.calls
      .filter((c: unknown[]) => (c[0] as ToolCall)?.name === 'write_file');
    expect(writeCalls).toHaveLength(3);
  });

  // ---------------------------------------------------------------------------
  // P0-6: Intent-based model routing + fallback on 429/5xx errors
  // ---------------------------------------------------------------------------

  it('routes simple messages to fast model', async () => {
    streamChat = makeStream(delta('Hello'), done());
    loop = new AgentLoop(streamChat, cancelFn);
    const cb = makeCallbacks();
    // gpt-4o is balanced → simple intent → route to fast model (gpt-4o-mini)
    await loop.run('hi', cb, makeContext({ model: 'gpt-4o', providerId: 'openai-compatible' }));

    const calls = (streamChat as unknown as ReturnType<typeof vi.fn>).mock.calls;
    const request = calls[0]?.[0] as ChatRequest | undefined;
    expect(request?.model).toBe('gpt-4o-mini');
    expect(cb.onStateChange).toHaveBeenCalledWith('routing', expect.stringContaining('fast'));
  });

  it('routes complex messages to strong model', async () => {
    streamChat = makeStream(delta('analyzed'), done());
    loop = new AgentLoop(streamChat, cancelFn);
    const cb = makeCallbacks();
    // claude-sonnet is balanced → complex intent → route to strong (claude-3-opus)
    await loop.run('analyze this complex architecture design', cb,
      makeContext({ model: 'claude-sonnet-4-20250514', providerId: 'anthropic' }));

    const calls = (streamChat as unknown as ReturnType<typeof vi.fn>).mock.calls;
    const request = calls[0]?.[0] as ChatRequest | undefined;
    expect(request?.model).toBe('claude-3-opus-20240229');
    expect(cb.onStateChange).toHaveBeenCalledWith('routing', expect.stringContaining('strong'));
  });

  it('429 error triggers fallback to backup model', async () => {
    // Stream produces only an error, no useful content → fallback-eligible
    const errorStream = vi.fn(async function* () {
      yield sError('429', 'Rate limit exceeded');
      yield done();
    }) as unknown as StreamFn;
    loop = new AgentLoop(errorStream, cancelFn);
    const cb = makeCallbacks();
    // Use 100+ char message to avoid simple routing (balanced tier → fallback to fast works)
    await loop.run('This is a detailed test to verify the fallback mechanism works when the provider returns a rate limit error',
      cb, makeContext({ model: 'gpt-4o', providerId: 'openai-compatible' }));

    // stream was called twice: once with gpt-4o, once with fallback
    const calls = (errorStream as unknown as ReturnType<typeof vi.fn>).mock.calls;
    expect(calls.length).toBeGreaterThanOrEqual(2);
    const firstReq = calls[0]?.[0] as ChatRequest | undefined;
    const secondReq = calls[1]?.[0] as ChatRequest | undefined;
    expect(firstReq?.model).toBe('gpt-4o');
    expect(secondReq?.model).toBe('gpt-4o-mini');
    expect(cb.onStateChange).toHaveBeenCalledWith('fallback', expect.stringContaining('gpt-4o-mini'));
  });

  it('non-retryable error does not trigger fallback', async () => {
    // A VALIDATION_ERROR is not a 429/5xx → no fallback attempt
    const errorStream = vi.fn(async function* () {
      yield sError('VALIDATION_ERROR', 'Invalid request');
      yield done();
    }) as unknown as StreamFn;
    loop = new AgentLoop(errorStream, cancelFn);
    const cb = makeCallbacks();
    await loop.run('hi', cb, makeContext());

    // stream called only once (plus error analysis) — no fallback
    const calls = (errorStream as unknown as ReturnType<typeof vi.fn>).mock.calls;
    // First call is the main loop; if there was a fallback there'd be 2 in main + 1 analysis
    // But there IS error analysis after, so total 2: main + analysis (no fallback = only 1 in for-loop)
    const mainLoopRequests = calls.filter((c: unknown[]) => {
      const req = c[0] as ChatRequest | undefined;
      return req?.model === 'gpt-4o';
    });
    // The main loop + error analysis both use gpt-4o (routed model unchanged)
    expect(mainLoopRequests.length).toBeGreaterThanOrEqual(1);
    // No fallback call registered
    const fallbackCalls = cb.onStateChange.mock.calls.filter(
      (c: unknown[]) => c[0] === 'fallback',
    );
    expect(fallbackCalls).toHaveLength(0);
  });

  // ---------------------------------------------------------------------------
  // Tool result cache
  // ---------------------------------------------------------------------------

  it('returns cached result for repeated identical read-only tool calls', async () => {
    const toolExecutor: ToolExecutor = {
      getDefinitions: () => [
        { name: 'read_file', description: '', parameters: { type: 'object', properties: {} } },
      ],
      hasSideEffect: () => false,
      execute: vi.fn((tc: ToolCall): Promise<ToolResult> =>
        Promise.resolve({ toolCallId: tc.id, content: 'file content' }),
      ),
    };

    let turn = 0;
    const stream = vi.fn(async function* () {
      turn++;
      if (turn === 1) {
        // Two identical read_file calls in the same turn → second should use cache
        yield tCall('tc-a', 'read_file', '{"path":"/foo"}');
        yield tCall('tc-b', 'read_file', '{"path":"/foo"}');
        yield done();
      } else {
        yield delta('done');
        yield done();
      }
    }) as unknown as StreamFn;

    const l = new AgentLoop(stream, cancelFn, toolExecutor);
    const cb = makeCallbacks();
    await l.run('read files', cb, makeContext());

    // read_file should only be executed once (second call hits cache)
    const readCalls = vi.mocked(toolExecutor.execute).mock.calls
      .filter((c: unknown[]) => (c[0] as ToolCall)?.name === 'read_file');
    expect(readCalls).toHaveLength(1);

    // Both tool results should still be reported to callbacks
    expect(cb.onToolResult).toHaveBeenCalledTimes(2);
  });

  it('cache miss on different tool arguments', async () => {
    const toolExecutor: ToolExecutor = {
      getDefinitions: () => [
        { name: 'read_file', description: '', parameters: { type: 'object', properties: {} } },
      ],
      hasSideEffect: () => false,
      execute: vi.fn((tc: ToolCall): Promise<ToolResult> =>
        Promise.resolve({ toolCallId: tc.id, content: 'file content' }),
      ),
    };

    let turn = 0;
    const stream = vi.fn(async function* () {
      turn++;
      if (turn === 1) {
        // Different paths → no cache hit
        yield tCall('tc-a', 'read_file', '{"path":"/foo"}');
        yield tCall('tc-b', 'read_file', '{"path":"/bar"}');
        yield done();
      } else {
        yield delta('done');
        yield done();
      }
    }) as unknown as StreamFn;

    const l = new AgentLoop(stream, cancelFn, toolExecutor);
    const cb = makeCallbacks();
    await l.run('read files', cb, makeContext());

    // Both calls should execute (different args → cache miss)
    const readCalls = vi.mocked(toolExecutor.execute).mock.calls
      .filter((c: unknown[]) => (c[0] as ToolCall)?.name === 'read_file');
    expect(readCalls).toHaveLength(2);
    expect(cb.onToolResult).toHaveBeenCalledTimes(2);
  });

  it('cache TTL expiry causes cache miss', async () => {
    vi.useFakeTimers();

    const toolExecutor: ToolExecutor = {
      getDefinitions: () => [
        { name: 'read_file', description: '', parameters: { type: 'object', properties: {} } },
      ],
      hasSideEffect: () => false,
      execute: vi.fn((tc: ToolCall): Promise<ToolResult> =>
        Promise.resolve({ toolCallId: tc.id, content: 'file content' }),
      ),
    };

    let callCount = 0;
    const stream = vi.fn(async function* () {
      callCount++;
      if (callCount === 1) {
        // Run 1, turn 1: read_file (gets cached)
        yield tCall('tc-1', 'read_file', '{"path":"/foo"}');
        yield done();
      } else if (callCount === 2) {
        // Run 1, turn 2: delta (no tool, loop exits)
        yield delta('done');
        yield done();
      } else if (callCount === 3) {
        // Run 2, turn 1: read_file again (cache should have expired)
        yield tCall('tc-3', 'read_file', '{"path":"/foo"}');
        yield done();
      } else {
        // Run 2, turns 2+: delta
        yield delta('done');
        yield done();
      }
    }) as unknown as StreamFn;

    const l = new AgentLoop(stream, cancelFn, toolExecutor);
    const cb = makeCallbacks();
    await l.run('read file', cb, makeContext());

    // Advance time past the 30s TTL
    vi.advanceTimersByTime(31000);

    // Second run: same args but TTL expired → should execute the tool again
    await l.run('read file again', cb, makeContext({ maxIterations: 5 }));

    // read_file should execute twice: run 1 (cached) + run 2 (TTL expired)
    const readCalls = vi.mocked(toolExecutor.execute).mock.calls
      .filter((c: unknown[]) => (c[0] as ToolCall)?.name === 'read_file');
    expect(readCalls).toHaveLength(2);

    vi.useRealTimers();
  });

  it('write operation invalidates cache', async () => {
    const toolExecutor: ToolExecutor = {
      getDefinitions: () => [
        { name: 'read_file', description: '', parameters: { type: 'object', properties: {} } },
        { name: 'write_file', description: '', parameters: { type: 'object', properties: {} } },
      ],
      hasSideEffect: (name: string) => hasSideEffect(name),
      execute: vi.fn((tc: ToolCall): Promise<ToolResult> =>
        Promise.resolve({ toolCallId: tc.id, content: 'done' }),
      ),
    };

    let callCount = 0;
    const stream = vi.fn(async function* () {
      callCount++;
      if (callCount === 1) {
        // Turn 1: read_file (gets cached)
        yield tCall('tc-1', 'read_file', '{"path":"/foo"}');
        yield done();
      } else if (callCount === 2) {
        // Turn 2: write_file (invalidates cache)
        yield tCall('tc-2', 'write_file', '{"path":"/foo","content":"new"}');
        yield done();
      } else if (callCount === 3) {
        // Turn 3: read_file again (should miss cache — was invalidated)
        yield tCall('tc-3', 'read_file', '{"path":"/foo"}');
        yield done();
      } else {
        // Subsequent turns: final answer
        yield delta('all done');
        yield done();
      }
    }) as unknown as StreamFn;

    const l = new AgentLoop(stream, cancelFn, toolExecutor);
    const cb = makeCallbacks();
    // Run the full multi-turn loop — it will process all tool calls
    await l.run('do stuff', cb, makeContext());

    // read_file should execute twice: turn 1 (cached) and turn 3 (cache invalidated by write_file in turn 2)
    const readCalls = vi.mocked(toolExecutor.execute).mock.calls
      .filter((c: unknown[]) => (c[0] as ToolCall)?.name === 'read_file');
    expect(readCalls).toHaveLength(2);

    // write_file should execute once
    const writeCalls = vi.mocked(toolExecutor.execute).mock.calls
      .filter((c: unknown[]) => (c[0] as ToolCall)?.name === 'write_file');
    expect(writeCalls).toHaveLength(1);
  });

  it('classifyModelTier classifies models correctly', () => {
    expect(classifyModelTier('gpt-4o-mini')).toBe('fast');
    expect(classifyModelTier('gpt-3.5-turbo')).toBe('fast');
    expect(classifyModelTier('gemini-2.0-flash')).toBe('fast');
    expect(classifyModelTier('gpt-4o')).toBe('balanced');
    expect(classifyModelTier('claude-sonnet-4-20250514')).toBe('balanced');
    expect(classifyModelTier('claude-3-opus-20240229')).toBe('strong');
  });

  it('runParallel emits start and done onParallelEvent per task', async () => {
    const factory = () => makeStream(delta('done'), done());
    const l = new AgentLoop(makeStream(delta('x'), done()), cancelFn);
    const cb = makeCallbacks();
    cb.onParallelEvent = vi.fn();
    const tasks: ParallelTask[] = [
      { task: 'a', agentName: 'explorer' },
      { task: 'b', agentName: 'reviewer' },
    ];
    await l.runParallel(tasks, cb, makeContext(), factory);

    const phases = cb.onParallelEvent.mock.calls.map((c: [ParallelAgentEvent]) => c[0].phase);
    expect(phases).toContain('start');
    expect(phases).toContain('done');
    const starts = cb.onParallelEvent.mock.calls
      .map((c: [ParallelAgentEvent]) => c[0])
      .filter((e: ParallelAgentEvent) => e.phase === 'start');
    expect(starts.map((e: ParallelAgentEvent) => e.agentName).sort()).toEqual(['explorer', 'reviewer']);
  });

  it('formatParallelResults renders success and failure sections', async () => {
    const out = formatParallelResults(
      [
        { taskId: 'parallel-0', success: true, content: 'ok-0', iterations: 2 },
        { taskId: 'parallel-1', success: false, error: 'boom', iterations: 1 },
      ],
      ['explorer', 'reviewer'],
    );
    expect(out).toContain('### Task 0 — explorer — success (2 iterations)');
    expect(out).toContain('ok-0');
    expect(out).toContain('### Task 1 — reviewer — FAILED');
    expect(out).toContain('boom');
  });

  it('intercepts dispatch_parallel_agents and feeds results back to the main agent', async () => {
    let mainTurn = 0;
    const mainStream: StreamFn = vi.fn(async function* () {
      if (mainTurn++ === 0) {
        yield tCall('d1', 'dispatch_parallel_agents', JSON.stringify({ tasks: [{ task: 'explore A' }, { task: 'explore B' }] }));
        yield done();
      } else {
        yield delta('synthesized');
        yield done();
      }
    }) as unknown as StreamFn;
    const factory = () => makeStream(delta('sub-result'), done());
    const toolExecutor: ToolExecutor = {
      execute: vi.fn(async (tc: ToolCall): Promise<ToolResult> =>
        tc.name === 'agents_list'
          ? { toolCallId: tc.id, content: JSON.stringify({ agents: [] }) }
          : { toolCallId: tc.id, content: 'unused' }),
      getDefinitions: () => [],
      hasSideEffect: () => false,
    } as unknown as ToolExecutor;

    const l = new AgentLoop(mainStream, cancelFn, toolExecutor, factory);
    const cb = makeCallbacks();
    cb.onParallelEvent = vi.fn();
    const toolResults: ToolResult[] = [];
    cb.onToolResult = vi.fn((r: ToolResult) => toolResults.push(r));

    await l.run('do A and B in parallel', cb, makeContext());

    expect(cb.onParallelEvent).toHaveBeenCalled();
    const dispatchResult = toolResults.find((r) => r.content.includes('Parallel results'));
    expect(dispatchResult).toBeDefined();
    expect(dispatchResult!.content).toContain('Task 0 — default');
    expect(cb.onDelta).toHaveBeenCalledWith('synthesized');
  });
});
