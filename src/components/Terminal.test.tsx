// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen } from '@testing-library/react';

vi.mock('../utils/ipc', () => ({
  typedInvoke: vi.fn(() => Promise.resolve('session-1')),
  normalizeError: (e: unknown) => ({ message: String(e) }),
}));

import { Terminal } from './Terminal';

beforeEach(() => {
  vi.clearAllMocks();
});

describe('Terminal', () => {
  it('renders the welcome line when there is no output yet', () => {
    render(<Terminal workspacePath="/tmp/ws" onClose={vi.fn()} />);
    expect(
      screen.getByText('Terminal session started. Click here and type commands.'),
    ).toBeTruthy();
  });
});
