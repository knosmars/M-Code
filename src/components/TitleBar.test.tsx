// @vitest-environment jsdom
import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';

vi.mock('@tauri-apps/api/window', () => ({
  getCurrentWindow: () => ({
    minimize: vi.fn(),
    toggleMaximize: vi.fn(),
    close: vi.fn(),
  }),
}));

import { TitleBar } from './TitleBar';

describe('TitleBar', () => {
  it('renders the toolbar with the menu button', () => {
    render(<TitleBar />);
    expect(screen.getByLabelText('Menu')).toBeTruthy();
  });

  it('renders the sidebar toggle', () => {
    render(<TitleBar />);
    expect(screen.getByLabelText('Toggle sidebar')).toBeTruthy();
  });
});
