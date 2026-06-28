// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, screen, waitFor, fireEvent } from '@testing-library/react';
import { useSettingsStore } from '../../stores/settingsStore';

const typedInvoke = vi.fn();
vi.mock('../../utils/ipc', () => ({
  typedInvoke: (...a: unknown[]) => typedInvoke(...a),
  normalizeError: (e: unknown) => ({ message: String(e) }),
}));

import { SemanticIndexSection } from './SemanticIndexSection';

beforeEach(() => {
  typedInvoke.mockReset();
});

afterEach(() => {
  useSettingsStore.getState().setLanguage('zh');
});

describe('SemanticIndexSection', () => {
  it('shows indexed status from tool_semantic_status', async () => {
    typedInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'tool_semantic_status') return Promise.resolve({ indexed: true, file_count: 3, chunk_count: 12, embed_model: 'nomic-embed-text', embed_dim: 768 });
      if (cmd === 'tool_semantic_config_get') return Promise.resolve({ embed_base: 'http://localhost:11434', embed_model: 'nomic-embed-text' });
      return Promise.resolve(undefined);
    });
    render(<SemanticIndexSection />);
    await waitFor(() => expect(screen.getByText('语义索引')).toBeTruthy());
    expect(typedInvoke).toHaveBeenCalledWith('tool_semantic_status', { path: '.' });
    await waitFor(() => expect(screen.getByText(/12/)).toBeTruthy());
  });

  it('shows a not-indexed state', async () => {
    typedInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'tool_semantic_status') return Promise.resolve({ indexed: false, file_count: 0, chunk_count: 0, embed_model: null, embed_dim: null });
      if (cmd === 'tool_semantic_config_get') return Promise.resolve({ embed_base: 'http://localhost:11434', embed_model: 'nomic-embed-text' });
      return Promise.resolve(undefined);
    });
    render(<SemanticIndexSection />);
    await waitFor(() => expect(screen.getByRole('button')).toBeTruthy());
  });

  it('loads and saves embedding config', async () => {
    typedInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'tool_semantic_status') return Promise.resolve({ indexed: false, file_count: 0, chunk_count: 0, embed_model: null, embed_dim: null });
      if (cmd === 'tool_semantic_config_get') return Promise.resolve({ embed_base: 'http://localhost:11434', embed_model: 'nomic-embed-text' });
      if (cmd === 'tool_semantic_config_set') return Promise.resolve(undefined);
      return Promise.resolve(undefined);
    });
    const { findByDisplayValue } = render(<SemanticIndexSection />);
    expect(await findByDisplayValue('nomic-embed-text')).toBeTruthy();
    expect(typedInvoke).toHaveBeenCalledWith('tool_semantic_config_get', {});
  });

  it('reflects and toggles the auto-index setting', async () => {
    typedInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'tool_semantic_status') return Promise.resolve({ indexed: false, file_count: 0, chunk_count: 0, embed_model: null, embed_dim: null });
      if (cmd === 'tool_semantic_config_get') return Promise.resolve({ embed_base: 'http://localhost:11434', embed_model: 'nomic-embed-text' });
      return Promise.resolve(undefined);
    });
    useSettingsStore.getState().setAutoSemanticIndex(false);
    render(<SemanticIndexSection />);
    const checkbox = (await screen.findByRole('checkbox')) as HTMLInputElement;
    expect(checkbox.checked).toBe(false);
    fireEvent.click(checkbox);
    expect(useSettingsStore.getState().autoSemanticIndex).toBe(true);
  });

  it('shows translated title in English and Chinese', async () => {
    typedInvoke.mockImplementation((cmd: string) => {
      if (cmd === 'tool_semantic_status') return Promise.resolve({ indexed: false, file_count: 0, chunk_count: 0, embed_model: null, embed_dim: null });
      if (cmd === 'tool_semantic_config_get') return Promise.resolve({ embed_base: 'http://localhost:11434', embed_model: 'nomic-embed-text' });
      return Promise.resolve(undefined);
    });

    // Test English
    useSettingsStore.getState().setLanguage('en');
    const { unmount } = render(<SemanticIndexSection />);
    await waitFor(() => expect(screen.getByText('Semantic index')).toBeTruthy());
    unmount();

    // Test Chinese
    useSettingsStore.getState().setLanguage('zh');
    render(<SemanticIndexSection />);
    await waitFor(() => expect(screen.getByText('语义索引')).toBeTruthy());
  });
});
