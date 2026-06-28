// @vitest-environment jsdom
import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { SlashCommandMenu } from './SlashCommandMenu';

describe('SlashCommandMenu', () => {
  it('lists commands and selects on mousedown', () => {
    const onSelect = vi.fn();
    render(
      <SlashCommandMenu
        items={[{ command: '/fix', labelKey: 'slash.fix' }, { command: '/test', labelKey: 'slash.test' }]}
        activeIndex={0}
        onSelect={onSelect}
        onHover={vi.fn()}
        t={(k) => k}
      />,
    );
    expect(screen.getByText('/fix')).toBeTruthy();
    expect(screen.getByText('/test')).toBeTruthy();
    fireEvent.mouseDown(screen.getByText('/test'));
    expect(onSelect).toHaveBeenCalledWith(1);
  });
});
