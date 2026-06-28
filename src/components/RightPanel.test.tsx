// @vitest-environment jsdom
import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { RightPanel } from './RightPanel';

describe('RightPanel', () => {
  it('shows the file path when given', () => {
    render(<RightPanel filePath="src/foo.ts" onClose={vi.fn()} />);
    expect(screen.getByText('src/foo.ts')).toBeTruthy();
  });

  it('shows the no-file placeholder when filePath is null', () => {
    render(<RightPanel filePath={null} onClose={vi.fn()} />);
    expect(screen.getByText('No file selected')).toBeTruthy();
  });

  it('shows the empty-body placeholder when no children', () => {
    render(<RightPanel filePath={null} onClose={vi.fn()} />);
    expect(screen.getByText('Select a file to preview')).toBeTruthy();
  });
});
