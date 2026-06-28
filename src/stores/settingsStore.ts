import { create } from 'zustand';
import type { Language } from '../i18n/translations';

type Theme = 'light' | 'dark' | 'system';

export interface SettingsState {
  /** Color theme preference */
  theme: Theme;
  /** Editor font size in pixels (default: 14) */
  fontSize: number;
  /** Whether to show timestamps on messages (default: true) */
  showTimestamps: boolean;
  /** Whether chat view auto-scrolls to new messages (default: true) */
  autoScroll: boolean;
  /** Whether to hide tool stderr from chat messages (default: false) */
  hideToolStderr: boolean;
  /** Whether to show tool call activity in assistant messages (default: true) */
  showToolCalls: boolean;
  /** UI language (default: zh) */
  language: Language;
  /** Whether to auto-index the workspace into the vector store in the background (default: false) */
  autoSemanticIndex: boolean;
  /** User-starred model names */
  favoriteModels: string[];
  /** Set the color theme */
  setTheme: (theme: Theme) => void;
  /** Set the editor font size */
  setFontSize: (size: number) => void;
  /** Toggle message timestamp visibility */
  setShowTimestamps: (show: boolean) => void;
  /** Toggle auto-scroll behavior */
  setAutoScroll: (enabled: boolean) => void;
  /** Toggle tool stderr visibility */
  setHideToolStderr: (hide: boolean) => void;
  /** Toggle tool call activity visibility */
  setShowToolCalls: (show: boolean) => void;
  /** Set UI language */
  setLanguage: (lang: Language) => void;
  /** Toggle automatic background semantic indexing */
  setAutoSemanticIndex: (enabled: boolean) => void;
  /** Toggle a model as favorite (star/unstar) */
  toggleFavoriteModel: (model: string) => void;
}

/** Settings persisted across app restarts (everything except favorites,
 *  which keep their own legacy key for backward compatibility). */
type PersistedSettings = Pick<
  SettingsState,
  'theme' | 'fontSize' | 'showTimestamps' | 'autoScroll' | 'hideToolStderr' | 'showToolCalls' | 'language' | 'autoSemanticIndex'
>;

const SETTINGS_KEY = 'meyatu-settings';

const DEFAULTS: PersistedSettings = {
  theme: 'system',
  fontSize: 14,
  showTimestamps: true,
  autoScroll: true,
  hideToolStderr: false,
  showToolCalls: true,
  language: 'zh',
  autoSemanticIndex: false,
};

function loadSettings(): PersistedSettings {
  try {
    const raw = localStorage.getItem(SETTINGS_KEY);
    // Merge over defaults so newly added keys get sane values.
    return raw ? { ...DEFAULTS, ...JSON.parse(raw) } : { ...DEFAULTS };
  } catch {
    return { ...DEFAULTS };
  }
}

function saveSettings(s: PersistedSettings) {
  try {
    localStorage.setItem(SETTINGS_KEY, JSON.stringify(s));
  } catch {
    /* storage unavailable — non-fatal */
  }
}

function loadFavorites(): string[] {
  try {
    const raw = localStorage.getItem('meyatu-favorite-models');
    return raw ? JSON.parse(raw) : [];
  } catch {
    return [];
  }
}

function saveFavorites(models: string[]) {
  localStorage.setItem('meyatu-favorite-models', JSON.stringify(models));
}

export const useSettingsStore = create<SettingsState>((set, get) => {
  /** Apply a partial update and persist the durable settings blob. */
  const persist = (patch: Partial<PersistedSettings>) => {
    set(patch as Partial<SettingsState>);
    const s = get();
    saveSettings({
      theme: s.theme,
      fontSize: s.fontSize,
      showTimestamps: s.showTimestamps,
      autoScroll: s.autoScroll,
      hideToolStderr: s.hideToolStderr,
      showToolCalls: s.showToolCalls,
      language: s.language,
      autoSemanticIndex: s.autoSemanticIndex,
    });
  };

  return {
    ...loadSettings(),
    favoriteModels: loadFavorites(),

    setTheme: (theme) => persist({ theme }),

    setFontSize: (fontSize) => persist({ fontSize }),

    setShowTimestamps: (showTimestamps) => persist({ showTimestamps }),

    setAutoScroll: (autoScroll) => persist({ autoScroll }),

    setHideToolStderr: (hideToolStderr) => persist({ hideToolStderr }),

    setShowToolCalls: (showToolCalls) => persist({ showToolCalls }),

    setLanguage: (language) => persist({ language }),

    setAutoSemanticIndex: (autoSemanticIndex) => persist({ autoSemanticIndex }),

    toggleFavoriteModel: (model) => {
      const { favoriteModels } = get();
      const next = favoriteModels.includes(model)
        ? favoriteModels.filter((m) => m !== model)
        : [...favoriteModels, model];
      saveFavorites(next);
      set({ favoriteModels: next });
    },
  };
});
