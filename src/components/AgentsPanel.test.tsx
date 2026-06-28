// @vitest-environment jsdom
import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { AgentsPanel } from './AgentsPanel';
import { useSettingsStore } from '../stores/settingsStore';
import type { ParallelRunState } from '../agent/parallelEvents';

const run: ParallelRunState = {
  total: 2,
  doneCount: 1,
  agents: {
    'parallel-0': { taskId: 'parallel-0', agentName: 'explorer', status: 'running', currentTool: 'grep', iterations: 3 },
    'parallel-1': { taskId: 'parallel-1', agentName: 'reviewer', status: 'done', iterations: 2, resultSummary: 'ok' },
  },
};

describe('AgentsPanel', () => {
  it('renders a card per agent with status detail', () => {
    render(<AgentsPanel run={run} />);
    expect(screen.getByText('explorer')).toBeTruthy();
    expect(screen.getByText(/grep/)).toBeTruthy();
    expect(screen.getByText('reviewer')).toBeTruthy();
    expect(screen.getByText(/ok/)).toBeTruthy();
  });

  it('renders an empty hint when there is no active run', () => {
    render(<AgentsPanel run={null} />);
    expect(screen.getByText(/no parallel agents|无并行/i)).toBeTruthy();
  });

  it('renders empty state in English when language is en', () => {
    useSettingsStore.getState().setLanguage('en');
    render(<AgentsPanel run={null} />);
    expect(screen.getByText('No parallel agents running')).toBeTruthy();
    expect(screen.queryByText('无并行 agent')).toBeNull();
    useSettingsStore.getState().setLanguage('zh'); // cleanup
  });
});
