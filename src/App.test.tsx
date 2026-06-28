// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { render, screen, act } from '@testing-library/react';

vi.mock('./components/ChatWindow', () => ({ ChatWindow: () => <div>CHAT_VIEW</div> }));
vi.mock('./components/SettingsPanel', () => ({ SettingsPanel: () => <div>SETTINGS_VIEW</div> }));
vi.mock('./stores/providerStore', () => ({ useProviderStore: (sel: (s: { initialize: () => void }) => unknown) => sel({ initialize: () => {} }) }));
// jsdom does not provide matchMedia — needed by the App theme effect
if (!window.matchMedia) {
  Object.defineProperty(window, "matchMedia", {
    writable: true,
    value: vi.fn().mockReturnValue({ matches: false, addEventListener: vi.fn(), removeEventListener: vi.fn() }),
  });
}
vi.mock('./stores/settingsStore', () => ({ useSettingsStore: (sel: (s: { theme: string }) => unknown) => sel({ theme: 'system' }) }));

import App from './App';
import { useViewStore } from './stores/viewStore';

beforeEach(() => {
  useViewStore.setState({ view: 'chat', previous: 'chat' });
});

describe('App view switching', () => {
  it('renders chat by default', () => {
    render(<App />);
    expect(screen.getByText('CHAT_VIEW')).toBeTruthy();
  });
  it('renders settings after navigate', () => {
    render(<App />);
    act(() => useViewStore.getState().navigate('settings'));
    expect(screen.getByText('SETTINGS_VIEW')).toBeTruthy();
  });
});
