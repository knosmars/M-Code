import ReactMarkdown from 'react-markdown';
import styles from './MarkdownRenderer.module.css';
import remarkGfm from 'remark-gfm';
import rehypeHighlight from 'rehype-highlight';
import { useState, useCallback, useRef } from 'react';
import type { ComponentPropsWithoutRef } from 'react';

interface MarkdownRendererProps {
  /** Raw markdown content to render. */
  content: string;
}

/**
 * Renders markdown content with GFM support and syntax-highlighted code blocks.
 *
 * Uses react-markdown v10 (children-based component API) with:
 * - remark-gfm: tables, strikethrough, task lists, autolinks
 * - rehype-highlight: syntax highlighting via highlight.js
 *
 * Code blocks receive a "Copy" button. Inline code is monospaced with accent styling.
 */
export function MarkdownRenderer({ content }: MarkdownRendererProps) {
  // Guard against non-string / empty content: react-markdown throws if its
  // children are not a string, which would crash the whole message bubble.
  const text = typeof content === 'string' ? content : String(content ?? '');
  if (text.trim() === '') {
    return null;
  }
  return (
    <div className={styles.markdownBody}>
    <ReactMarkdown
      remarkPlugins={[remarkGfm]}
      rehypePlugins={[rehypeHighlight]}
      components={{
        code: CodeBlock,
        pre: PreBlock,
      }}
    >
      {text}
    </ReactMarkdown>
    </div>
  );
}

/** Distinguish fenced code blocks from inline code using className. */
function CodeBlock({
  className,
  children,
  ...props
}: ComponentPropsWithoutRef<'code'>) {
  const match = /language-(\w+)/.exec(className ?? '');
  const isFenced = match !== null;
  const [copied, setCopied] = useState(false);
  const timerRef = useRef<ReturnType<typeof setTimeout>>(undefined);

  // Hooks must run unconditionally, so define these before the inline-code
  // early return (raw is cheap to compute even for inline code).
  const raw = extractText(children);
  const handleCopy = useCallback(() => {
    navigator.clipboard.writeText(raw).then(() => {
      setCopied(true);
      clearTimeout(timerRef.current);
      timerRef.current = setTimeout(() => setCopied(false), 2000);
    }).catch(() => {
      // Silently ignore clipboard failures (e.g. in non-HTTPS dev).
    });
  }, [raw]);

  if (!isFenced) {
    // Inline code
    return (
      <code className={styles.inlineCode} {...props}>
        {children}
      </code>
    );
  }

  const language = match[1];

  return (
    <div className={styles.codeBlock}>
      <div className={styles.header}>
        <span className={styles.lang}>{language}</span>
        <button
          type="button"
          className={`${styles.copy}${copied ? ' ' + styles['copy--done'] : ''}`}
          aria-label={copied ? 'Copied!' : `Copy ${language} code`}
          onClick={handleCopy}
        >
          {copied ? '✓ Copied' : 'Copy'}
        </button>
      </div>
      <code className={className} {...props}>
        {children}
      </code>
    </div>
  );
}

/** Strip wrapping <pre> styling since we wrap code in .code-block. */
function PreBlock({
  children,
  ...props
}: ComponentPropsWithoutRef<'pre'>) {
  return <pre {...props}>{children}</pre>;
}

// ---- helpers ----

function extractText(children: React.ReactNode): string {
  if (typeof children === 'string') return children;
  if (Array.isArray(children)) return Array.from(children).map((c) => (typeof c === 'string' ? c : '')).join('');
  return '';
}
