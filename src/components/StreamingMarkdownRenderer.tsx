import { useState, useEffect, useRef, useMemo, startTransition } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import rehypeHighlight from 'rehype-highlight';

interface StreamingMarkdownRendererProps {
  /** The markdown content to display (grows incrementally during streaming). */
  content: string;
  className?: string;
}

/**
 * Progressively renders markdown during streaming.
 *
 * **Delta buffering** avoids re-parsing the entire document on every keystroke:
 *  - *Time-based* — stream finished (content unchanged for 600 ms) → committed.
 *  - *Size-based* — ≥ 8 KB accumulated since last commit → immediate commit.
 *
 * **Stable prefix vs pending buffer** — the stable prefix is rendered as full
 * markdown (react-markdown + remark-gfm + rehype-highlight). The pending buffer
 * (recent deltas not yet committed) is rendered as plain text with special
 * handling for unclosed syntax to prevent visual artifacts:
 *  - Odd `` ``` `` fence count in full content → inside code block → `<pre>`.
 *  - Odd `**` count in pending → unclosed bold → literal `<span>`.
 *  - More `[` than `]` in pending → unclosed link → literal `<span>`.
 *  - Last line with odd pipe count → incomplete table row → literal `<span>`.
 *
 * **Flicker prevention** — timer stored in `useRef` (no new timer objects per
 * render); stable/prefix split computed via `useMemo` for identity stability.
 */
export function StreamingMarkdownRenderer({
  content,
  className,
}: StreamingMarkdownRendererProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Number of characters from the start of `content` that are "stable"
  // (rendered as full markdown). Everything after is the pending buffer.
  const [committedLength, setCommittedLength] = useState(0);

  // --- auto-scroll (preserves StreamingText behaviour) -----------------------
  useEffect(() => {
    const container = containerRef.current;
    if (container === null) return;
    let element: HTMLElement | null = container.parentElement;
    while (element !== null) {
      const overflowY = window.getComputedStyle(element).overflowY;
      if (overflowY === 'auto' || overflowY === 'scroll') {
        element.scrollTop = element.scrollHeight;
        break;
      }
      element = element.parentElement;
    }
  }, [content]);

  // --- time-based flush (600 ms inactivity) ----------------------------------
  useEffect(() => {
    if (timerRef.current !== null) {
      clearTimeout(timerRef.current);
    }
    timerRef.current = setTimeout(() => {
      startTransition(() => {
        setCommittedLength(content.length);
      });
    }, 600);
    return () => {
      if (timerRef.current !== null) {
        clearTimeout(timerRef.current);
      }
    };
  }, [content]);

  // --- size-based flush (≥ 8 KB accumulated) ---------------------------------
  useEffect(() => {
    const delta = content.length - committedLength;
    if (delta >= 8192) {
      if (timerRef.current !== null) {
        clearTimeout(timerRef.current);
      }
      startTransition(() => {
        setCommittedLength(content.length);
      });
    }
  }, [content, committedLength]);

  // --- stable prefix / pending buffer (memoised for identity) ----------------
  const stable = useMemo(
    () => content.slice(0, committedLength),
    [content, committedLength],
  );

  const pending = useMemo(
    () => content.slice(committedLength),
    [content, committedLength],
  );

  // --- pending render element (unclosed-syntax aware) ------------------------
  const pendingElement = useMemo(() => {
    if (pending.length === 0) return null;

    // 1. Code fences — count in FULL content to detect if we are *inside* a
    //    fenced block that started in the stable prefix.
    const fenceCount = (content.match(/```/g) || []).length;
    if (fenceCount % 2 === 1) {
      return <pre className="streaming-pending">{pending}</pre>;
    }

    // 2. Unclosed bold (`**`) in pending buffer.
    const boldCount = (pending.match(/\*\*/g) || []).length;
    if (boldCount % 2 === 1) {
      return <span className="streaming-pending">{pending}</span>;
    }

    // 3. Unclosed link bracket (`[`) in pending buffer.
    const openBrackets = (pending.match(/\[/g) || []).length;
    const closeBrackets = (pending.match(/]/g) || []).length;
    if (openBrackets > closeBrackets) {
      return <span className="streaming-pending">{pending}</span>;
    }

    // 4. Incomplete table row — last line with odd pipe count means the row is
    //    still being typed (a complete GFM row has an even number of `|`).
    const lines = pending.split('\n');
    const lastLine = lines[lines.length - 1] ?? '';
    if (lastLine.includes('|')) {
      const pipes = (lastLine.match(/\|/g) || []).length;
      if (pipes % 2 === 1) {
        return <span className="streaming-pending">{pending}</span>;
      }
    }

    // 5. Default — render as plain text span.
    return <span className="streaming-pending">{pending}</span>;
  }, [pending, content]);

  // --- empty content early exit ----------------------------------------------
  if (content.length === 0) {
    return <div ref={containerRef} className={className} />;
  }

  return (
    <div ref={containerRef} className={className} style={{ fontFamily: 'var(--font-prose)', fontSize: 'var(--font-size-prose)', lineHeight: 'var(--line-height-prose)' }}>
      {stable.length > 0 && (
        <ReactMarkdown
          remarkPlugins={[remarkGfm]}
          rehypePlugins={[rehypeHighlight]}
        >
          {stable}
        </ReactMarkdown>
      )}
      {pendingElement}
    </div>
  );
}
