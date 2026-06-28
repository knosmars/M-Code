import { useCallback } from 'react';
import { useSettingsStore } from '../stores/settingsStore';
import { translations, type TranslationKey } from './translations';

export function useT() {
  const language = useSettingsStore((s) => s.language);
  const t = useCallback(
    (key: TranslationKey, params?: Record<string, string | number>) => {
      let s: string = translations[language][key] ?? translations.en[key] ?? key;
      if (params) {
        for (const [k, v] of Object.entries(params)) {
          s = s.replaceAll(`{${k}}`, String(v));
        }
      }
      return s;
    },
    [language],
  );
  return t;
}
