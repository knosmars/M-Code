import type { ReactNode } from 'react';
import { BrandIcon } from './BrandIcon';
import styles from './WelcomeScreen.module.css';

interface CapabilityCard {
  id: string;
  title: string;
  prompt: string;
  icon: ReactNode;
}

// Six capabilities that distinguish Meyatu from Claude Code / OpenAI Codex.
// Copy is hardcoded zh — move to useT/translation keys if EN is ever needed.
const CARDS: CapabilityCard[] = [
  { id: 'ssh', title: 'SSH 远程', prompt: 'SSH 到生产服务器，查 nginx 为什么 502', icon: '🔌' },
  { id: 'web', title: '网页研究', prompt: '爬这个文档站，整理出 API 用法', icon: '🌐' },
  { id: 'semantic', title: '语义检索', prompt: '用语义搜索找出处理支付的代码', icon: '🔍' },
  { id: 'impact', title: '影响分析', prompt: '我要改这个函数，先分析改动波及面', icon: '🕸️' },
  { id: 'automation', title: '自演化自动化', prompt: '配个触发器：提交前自动跑测试', icon: '⚙️' },
  { id: 'global', title: '全局知识库', prompt: '在我所有项目里搜之前用过的鉴权方案', icon: '🗂️' },
];

function greeting(date = new Date()): string {
  const h = date.getHours();
  if (h < 6) return '夜深了';
  if (h < 12) return '早上好';
  if (h < 18) return '下午好';
  return '晚上好';
}

interface WelcomeScreenProps {
  /** Fill the composer with the chosen example prompt (editable, not sent). */
  onPickPrompt: (text: string) => void;
}

export function WelcomeScreen({ onPickPrompt }: WelcomeScreenProps) {
  return (
    <div className={styles.welcome}>
      <div className={styles.inner}>
        <h1 className={styles.heading}>
          <BrandIcon size={28} className={styles.brandIcon} />
          {greeting()}，我是 Meyatu Code
        </h1>
        <p className={styles.tagline}>
          自主开发伙伴 — 能改代码、跑远程、爬网页，还会给自己配自动化
        </p>

        <div className={styles.cardGrid}>
          {CARDS.map((c) => (
            <button
              key={c.id}
              type="button"
              className={styles.card}
              onClick={() => onPickPrompt(c.prompt)}
            >
              <span className={styles.cardIcon} aria-hidden="true">{c.icon}</span>
              <span className={styles.cardTitle}>{c.title}</span>
              <span className={styles.cardPrompt}>{c.prompt}</span>
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}
