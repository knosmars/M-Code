import { describe, it, expect } from 'vitest';
import { translations } from './translations';

describe('translations parity', () => {
  it('en and zh have identical key sets', () => {
    const en = Object.keys(translations.en).sort();
    const zh = Object.keys(translations.zh).sort();
    expect(zh).toEqual(en);
  });

  it('has the batch-1 migrated keys with expected values', () => {
    expect(translations.zh['composer.offline']).toBe('离线 — 无法发送，请检查网络');
    expect(translations.en['composer.offline']).toBe('Offline — cannot send, check your network');
    expect(translations.zh['settings.connection.test']).toBe('测试连接');
    expect(translations.en['settings.connection.test']).toBe('Test connection');
    expect(translations.zh['chat.error.rateLimited']).toBe('请求过于频繁');
    expect(translations.en['chat.error.rateLimited']).toBe('Too many requests');
  });
});
