// @vitest-environment jsdom
import { describe, it, expect, vi } from 'vitest';
import { render, screen } from '@testing-library/react';
import { SessionList } from './SessionList';

function renderList() {
  return render(
    <SessionList
      sessions={[]}
      currentSessionId={null}
      providers={[]}
      onSelect={vi.fn()}
      onNew={vi.fn()}
      onDelete={vi.fn()}
      onRename={vi.fn()}
      onCustomize={vi.fn()}
      focusSearchKey={0}
      sidebarMode="sessions"
      onToggleSidebarMode={vi.fn()}
    />,
  );
}

describe('SessionList', () => {
  it('renders the nav rail with the sidebar pane toggle', () => {
    renderList();
    expect(screen.getByLabelText('Sidebar pane')).toBeTruthy();
  });

  it('renders the search affordance', () => {
    renderList();
    expect(screen.getByLabelText('Search / sort sessions')).toBeTruthy();
  });
});
