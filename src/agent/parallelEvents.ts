/** Structured progress event for one parallel sub-agent. */
export interface ParallelAgentEvent {
  taskId: string;
  agentName: string;
  phase: 'start' | 'tool' | 'result' | 'done' | 'error';
  tool?: string;
  iteration?: number;
  summary?: string;
  error?: string;
}

/** Reduced state for a single parallel sub-agent. */
export interface ParallelAgentState {
  taskId: string;
  agentName: string;
  status: 'running' | 'done' | 'error';
  currentTool?: string;
  iterations: number;
  resultSummary?: string;
  error?: string;
}

/** Aggregate state for an in-flight (or finished) parallel run. */
export interface ParallelRunState {
  agents: Record<string, ParallelAgentState>;
  total: number;
  doneCount: number;
}

/** Pure reducer: fold a ParallelAgentEvent into the run state. */
export function reduceParallel(
  state: ParallelRunState | null,
  event: ParallelAgentEvent,
): ParallelRunState {
  const base: ParallelRunState = state ?? { agents: {}, total: 0, doneCount: 0 };
  const agents = { ...base.agents };

  const existing = agents[event.taskId];
  const wasTerminal = !!existing && existing.status !== 'running';

  const cur: ParallelAgentState = existing
    ? { ...existing }
    : {
        taskId: event.taskId,
        agentName: event.agentName,
        status: 'running',
        iterations: event.iteration ?? 0,
      };

  switch (event.phase) {
    case 'start':
      cur.status = 'running';
      break;
    case 'tool':
      cur.currentTool = event.tool;
      if (event.iteration !== undefined) cur.iterations = event.iteration;
      break;
    case 'result':
      if (event.iteration !== undefined) cur.iterations = event.iteration;
      break;
    case 'done':
      cur.status = 'done';
      cur.currentTool = undefined;
      cur.resultSummary = event.summary;
      if (event.iteration !== undefined) cur.iterations = event.iteration;
      break;
    case 'error':
      cur.status = 'error';
      cur.currentTool = undefined;
      cur.error = event.error;
      if (event.iteration !== undefined) cur.iterations = event.iteration;
      break;
  }

  agents[event.taskId] = cur;
  const becameTerminal = !wasTerminal && cur.status !== 'running';
  return {
    agents,
    total: Object.keys(agents).length,
    doneCount: base.doneCount + (becameTerminal ? 1 : 0),
  };
}
