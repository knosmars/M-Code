// @vitest-environment jsdom
import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { ParallelAgentsCard } from './ParallelAgentsCard';
import { useSettingsStore } from '../stores/settingsStore';
import type { ParallelAgentState } from '../agent/parallelEvents';

const agents: ParallelAgentState[] = [
  { taskId: 'parallel-0', agentName: 'explorer', status: 'running', currentTool: 'read_file', iterations: 4 },
  { taskId: 'parallel-1', agentName: 'reviewer', status: 'done', iterations: 2, resultSummary: 'looks good' },
  { taskId: 'parallel-2', agentName: 'tester', status: 'error', iterations: 1, error: 'timeout' },
];

describe('ParallelAgentsCard', () => {
  it('renders a lane per agent with the X/N header', () => {
    render(<ParallelAgentsCard agents={agents} />);
    expect(screen.getByText('explorer')).toBeTruthy();
    expect(screen.getByText('reviewer')).toBeTruthy();
    expect(screen.getByText('tester')).toBeTruthy();
    expect(screen.getByText(/2\s*\/\s*3/)).toBeTruthy();
  });

  it('shows the running tool and the error', () => {
    render(<ParallelAgentsCard agents={agents} />);
    expect(screen.getByText(/read_file/)).toBeTruthy();
    expect(screen.getByText(/timeout/)).toBeTruthy();
  });

  it('renders title in English when language is en', () => {
    useSettingsStore.getState().setLanguage('en');
    render(<ParallelAgentsCard agents={agents} />);
    expect(screen.getByText(/Parallel agents/)).toBeTruthy();
    expect(screen.queryByText(/并行 agent/)).toBeNull();
    useSettingsStore.getState().setLanguage('zh'); // cleanup
  });
});
