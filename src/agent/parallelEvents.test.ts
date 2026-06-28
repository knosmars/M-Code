import { describe, it, expect } from 'vitest';
import { reduceParallel, type ParallelAgentEvent } from './parallelEvents';

const ev = (e: Partial<ParallelAgentEvent> & { taskId: string; phase: ParallelAgentEvent['phase'] }): ParallelAgentEvent => ({
  agentName: 'default',
  ...e,
});

describe('reduceParallel', () => {
  it('start creates a running agent and bumps total', () => {
    const s = reduceParallel(null, ev({ taskId: 'parallel-0', agentName: 'explorer', phase: 'start' }));
    expect(s.total).toBe(1);
    expect(s.doneCount).toBe(0);
    expect(s.agents['parallel-0']).toMatchObject({ agentName: 'explorer', status: 'running', iterations: 0 });
  });

  it('tool updates currentTool and iterations', () => {
    let s = reduceParallel(null, ev({ taskId: 'parallel-0', phase: 'start' }));
    s = reduceParallel(s, ev({ taskId: 'parallel-0', phase: 'tool', tool: 'read_file', iteration: 3 }));
    expect(s.agents['parallel-0']).toMatchObject({ currentTool: 'read_file', iterations: 3, status: 'running' });
  });

  it('done sets terminal status, clears tool, bumps doneCount once', () => {
    let s = reduceParallel(null, ev({ taskId: 'parallel-0', phase: 'start' }));
    s = reduceParallel(s, ev({ taskId: 'parallel-0', phase: 'tool', tool: 'grep', iteration: 1 }));
    s = reduceParallel(s, ev({ taskId: 'parallel-0', phase: 'done', summary: 'found 2', iteration: 2 }));
    expect(s.agents['parallel-0']).toMatchObject({ status: 'done', resultSummary: 'found 2', currentTool: undefined });
    expect(s.doneCount).toBe(1);
    s = reduceParallel(s, ev({ taskId: 'parallel-0', phase: 'result' }));
    expect(s.doneCount).toBe(1);
  });

  it('error sets error status + message', () => {
    let s = reduceParallel(null, ev({ taskId: 'parallel-1', phase: 'start' }));
    s = reduceParallel(s, ev({ taskId: 'parallel-1', phase: 'error', error: 'timeout' }));
    expect(s.agents['parallel-1']).toMatchObject({ status: 'error', error: 'timeout' });
    expect(s.doneCount).toBe(1);
  });

  it('isolates multiple agents', () => {
    let s = reduceParallel(null, ev({ taskId: 'parallel-0', phase: 'start' }));
    s = reduceParallel(s, ev({ taskId: 'parallel-1', phase: 'start' }));
    s = reduceParallel(s, ev({ taskId: 'parallel-0', phase: 'done', summary: 'ok' }));
    expect(s.total).toBe(2);
    expect(s.doneCount).toBe(1);
    expect(s.agents['parallel-1'].status).toBe('running');
  });

  it('self-initializes on an unknown taskId event', () => {
    const s = reduceParallel(null, ev({ taskId: 'parallel-9', phase: 'done', summary: 'late' }));
    expect(s.total).toBe(1);
    expect(s.doneCount).toBe(1);
    expect(s.agents['parallel-9'].status).toBe('done');
  });
});
