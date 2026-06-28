// @vitest-environment jsdom
import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { CodeEditor } from './CodeEditor';

describe('CodeEditor', () => {
  it('shows a loading state when content is null', () => {
    render(<CodeEditor filePath="a.ts" content={null} />);
    expect(screen.getByText('Loading…')).toBeTruthy();
  });

  it('renders the file name title and line stats for content', () => {
    render(<CodeEditor filePath="src/foo.ts" content={'line1\nline2\nline3'} />);
    expect(screen.getByText('foo.ts')).toBeTruthy();
    expect(screen.getByText('3 lines')).toBeTruthy();
  });
});
