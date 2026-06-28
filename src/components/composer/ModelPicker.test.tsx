// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { ModelPicker } from './ModelPicker';
import { useSettingsStore } from '../../stores/settingsStore';

describe('ModelPicker', () => {
  it('lists models and selects + closes on click', () => {
    const onSelect = vi.fn();
    const onClose = vi.fn();
    render(
      <ModelPicker
        models={['gpt-4o', 'gpt-4o-mini']}
        currentModel="gpt-4o"
        onSelect={onSelect}
        onClose={onClose}
        moreOpen={false}
        onToggleMore={vi.fn()}
      />,
    );
    expect(screen.getByText('gpt-4o-mini')).toBeTruthy();
    fireEvent.click(screen.getByText('gpt-4o-mini'));
    expect(onSelect).toHaveBeenCalledWith('gpt-4o-mini');
    expect(onClose).toHaveBeenCalled();
  });

  it('shows "More models" button in English when models.length > 6', () => {
    useSettingsStore.setState({ language: 'en' });
    const models = Array.from({ length: 10 }, (_, i) => `model-${i}`);
    render(
      <ModelPicker
        models={models}
        currentModel="model-0"
        onSelect={vi.fn()}
        onClose={vi.fn()}
        moreOpen={false}
        onToggleMore={vi.fn()}
      />,
    );
    expect(screen.getByText(/More models \(4\)/)).toBeTruthy();
    expect(screen.queryByText(/更多模型/)).toBeNull();
  });

  it('shows "更多模型" button in Chinese when models.length > 6', () => {
    useSettingsStore.setState({ language: 'zh' });
    const models = Array.from({ length: 10 }, (_, i) => `model-${i}`);
    render(
      <ModelPicker
        models={models}
        currentModel="model-0"
        onSelect={vi.fn()}
        onClose={vi.fn()}
        moreOpen={false}
        onToggleMore={vi.fn()}
      />,
    );
    expect(screen.getByText(/更多模型（4）/)).toBeTruthy();
    expect(screen.queryByText(/More models/)).toBeNull();
  });

  afterEach(() => {
    useSettingsStore.setState({ language: 'zh' });
  });
});
