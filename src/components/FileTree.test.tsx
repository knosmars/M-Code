// @vitest-environment jsdom
import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';

vi.mock('../utils/ipc', () => ({
  typedInvoke: vi.fn(() => Promise.resolve([])),
  normalizeError: (e: unknown) => ({ message: String(e) }),
}));

import { FileTree } from './FileTree';

describe('FileTree', () => {
  it('renders the Files section header', () => {
    render(<FileTree workspacePath="/tmp/ws" />);
    expect(screen.getByText('Files')).toBeTruthy();
  });
});
