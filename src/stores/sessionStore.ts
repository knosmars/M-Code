import { create } from 'zustand';
import { typedInvoke } from '../utils/ipc';
import { useToastStore } from './toastStore';
import type { Session, TokenUsage } from '../types/session';
import type { Message } from '../types/message';

export interface SessionState {
  sessions: Session[];
  currentSessionId: string | null;
  loaded: boolean;
  loadSessions: () => Promise<void>;
  createSession: (provider?: string, model?: string, workspacePath?: string) => string;
  deleteSession: (id: string) => void;
  setCurrentSession: (id: string | null) => void;
  renameSession: (sessionId: string, title: string) => void;
  addMessage: (sessionId: string, message: Message) => void;
  updateSessionStatus: (sessionId: string, status: Session['status']) => void;
  addTokens: (sessionId: string, tokens: TokenUsage) => void;
  getCurrentSession: () => Session | null;
}

const ZERO_TOKENS: TokenUsage = { promptTokens: 0, completionTokens: 0, totalTokens: 0, cost: 0 };

const MAX_PERSIST_RETRIES = 3;
const RETRY_BASE_DELAY_MS = 500;

async function persistWithRetry(
  operation: () => Promise<void>,
  label: string,
): Promise<void> {
  for (let attempt = 1; attempt <= MAX_PERSIST_RETRIES; attempt++) {
    try {
      await operation();
      return;
    } catch (e) {
      if (attempt === MAX_PERSIST_RETRIES) {
        console.error(`${label} failed after ${MAX_PERSIST_RETRIES} attempts:`, e);
        useToastStore.getState().addToast('error', '会话保存失败，更改可能丢失');
        return;
      }
      const delay = RETRY_BASE_DELAY_MS * Math.pow(2, attempt - 1);
      console.warn(`${label} attempt ${attempt} failed, retrying in ${delay}ms:`, e);
      await new Promise((resolve) => setTimeout(resolve, delay));
    }
  }
}

function persistSession(session: Session) {
  persistWithRetry(
    () => typedInvoke<void>('db_save_session', { json: JSON.stringify(session) }),
    'db_save_session',
  );
}

function persistDelete(id: string) {
  persistWithRetry(
    () => typedInvoke<void>('db_delete_session', { id }),
    'db_delete_session',
  );
}

export const useSessionStore = create<SessionState>((set, get) => ({
  sessions: [],
  currentSessionId: null,
  loaded: false,

  loadSessions: async () => {
    try {
      const json = await typedInvoke<string>('db_load_sessions');
      const sessions: Session[] = JSON.parse(json);
      set({ sessions, loaded: true });
    } catch (e) {
      console.error('loadSessions failed:', e);
      useToastStore.getState().addToast('error', '会话加载失败');
      set({ loaded: true });
    }
  },

  createSession: (provider, model, workspacePath?: string) => {
    const id = crypto.randomUUID();
    const now = Date.now();
    const newSession: Session = {
      id,
      title: 'New Chat',
      messages: [],
      provider,
      model,
      status: { type: 'idle' },
      tokens: { ...ZERO_TOKENS },
      workspacePath: workspacePath,
      createdAt: now,
      updatedAt: now,
    };
    set((state) => ({
      sessions: [...state.sessions, newSession],
      currentSessionId: id,
    }));
    persistSession(newSession);
    return id;
  },

  deleteSession: (id) => {
    set((state) => ({
      sessions: state.sessions.filter((s) => s.id !== id),
      currentSessionId: state.currentSessionId === id ? null : state.currentSessionId,
    }));
    persistDelete(id);
  },

  setCurrentSession: (id) => set({ currentSessionId: id }),

  renameSession: (sessionId, title) =>
    set((state) => {
      const sessions = state.sessions.map((s) =>
        s.id === sessionId ? { ...s, title, updatedAt: Date.now() } : s,
      );
      const updated = sessions.find((s) => s.id === sessionId);
      if (updated) persistSession(updated);
      return { sessions };
    }),

  addMessage: (sessionId, message) =>
    set((state) => {
      const sessions = state.sessions.map((s) =>
        s.id === sessionId
          ? { ...s, messages: [...s.messages, message], updatedAt: Date.now() }
          : s,
      );
      const updated = sessions.find((s) => s.id === sessionId);
      if (updated) persistSession(updated);
      return { sessions };
    }),

  updateSessionStatus: (sessionId, status) =>
    set((state) => {
      const sessions = state.sessions.map((s) =>
        s.id === sessionId ? { ...s, status, updatedAt: Date.now() } : s,
      );
      const updated = sessions.find((s) => s.id === sessionId);
      if (updated) persistSession(updated);
      return { sessions };
    }),

  addTokens: (sessionId, tokens) =>
    set((state) => {
      const sessions = state.sessions.map((s) =>
        s.id === sessionId
          ? {
              ...s,
              tokens: {
                promptTokens: s.tokens.promptTokens + tokens.promptTokens,
                completionTokens: s.tokens.completionTokens + tokens.completionTokens,
                totalTokens: s.tokens.totalTokens + tokens.totalTokens,
                cost: s.tokens.cost + tokens.cost,
              },
            }
          : s,
      );
      const updated = sessions.find((s) => s.id === sessionId);
      if (updated) persistSession(updated);
      return { sessions };
    }),

  getCurrentSession: () => {
    const { sessions, currentSessionId } = get();
    return sessions.find((s) => s.id === currentSessionId) ?? null;
  },
}));
