import { describe, it, expect, beforeEach } from 'vitest';
import { useViewStore } from './viewStore';

beforeEach(() => {
  useViewStore.setState({ view: 'chat', previous: 'chat' });
});

describe('viewStore', () => {
  it('defaults to chat', () => {
    expect(useViewStore.getState().view).toBe('chat');
  });
  it('navigate records previous and switches', () => {
    useViewStore.getState().navigate('settings');
    const s = useViewStore.getState();
    expect(s.view).toBe('settings');
    expect(s.previous).toBe('chat');
  });
  it('goBack returns to previous', () => {
    useViewStore.getState().navigate('settings');
    useViewStore.getState().goBack();
    expect(useViewStore.getState().view).toBe('chat');
  });
});
