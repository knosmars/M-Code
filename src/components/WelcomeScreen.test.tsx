// @vitest-environment jsdom
import { describe, it, expect, vi } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { WelcomeScreen } from './WelcomeScreen';

const PROMPTS = [
  'SSH 到生产服务器，查 nginx 为什么 502',
  '爬这个文档站，整理出 API 用法',
  '用语义搜索找出处理支付的代码',
  '我要改这个函数，先分析改动波及面',
  '配个触发器：提交前自动跑测试',
  '在我所有项目里搜之前用过的鉴权方案',
];

describe('WelcomeScreen', () => {
  it('renders all six capability example prompts', () => {
    render(<WelcomeScreen onPickPrompt={() => {}} />);
    for (const p of PROMPTS) {
      expect(screen.getByText(p)).toBeTruthy();
    }
  });

  it('calls onPickPrompt with the exact prompt when a card is clicked', () => {
    const onPick = vi.fn();
    render(<WelcomeScreen onPickPrompt={onPick} />);
    fireEvent.click(screen.getByText(PROMPTS[2]));
    expect(onPick).toHaveBeenCalledWith(PROMPTS[2]);
  });

  it('drops the Claude Code tells (no "What\'s up next?", no Gatsby)', () => {
    render(<WelcomeScreen onPickPrompt={() => {}} />);
    expect(screen.queryByText(/What's up next/i)).toBeNull();
    expect(screen.queryByText(/Gatsby/i)).toBeNull();
  });
});
