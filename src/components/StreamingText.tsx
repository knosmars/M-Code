import { useEffect, useRef } from 'react';
import styles from './StreamingText.module.css';

interface StreamingTextProps {
  /** The text content to display */
  text: string;
}

/**
 * Renders text content split into paragraphs.
 * Auto-scrolls the nearest scrollable ancestor to the bottom on content changes.
 */
export function StreamingText({ text }: StreamingTextProps) {
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const container = containerRef.current;
    if (container === null) {
      return;
    }
    // Find the nearest scrollable parent and scroll it to the bottom.
    let element: HTMLElement | null = container.parentElement;
    while (element !== null) {
      const overflowY = window.getComputedStyle(element).overflowY;
      if (overflowY === 'auto' || overflowY === 'scroll') {
        element.scrollTop = element.scrollHeight;
        break;
      }
      element = element.parentElement;
    }
  }, [text]);

  // Split text into paragraphs by double newlines so each segment renders
  // as a visually distinct block.
  const paragraphs = text.split(/\n\n+/).filter(Boolean);

  return (
    <div ref={containerRef} className={styles.streamingText}>
      {paragraphs.map((p, i) => (
        <p key={i} className="streaming-text__paragraph">
          {p}
        </p>
      ))}
    </div>
  );
}
