import { useState, useRef, useEffect, useCallback } from 'react';
import { typedInvoke, normalizeError } from '../utils/ipc';
import { useFileSyncStore } from '../stores/fileSyncStore';
import shared from './Terminal.module.css';

const ANSI_ESCAPE_RE = /\x1b\[[0-9;]*[mGKHFABCDJsu]|\x1b\][^\x07]*\x07|\x1b[()][AB012]/g;

function stripAnsi(s: string): string {
  return s.replace(ANSI_ESCAPE_RE, '');
}

interface TerminalProps {
  workspacePath: string;
  onClose: () => void;
  initialCommand?: string;
  sessionId?: string;
  onStarted?: () => void;
}

interface TermLine {
  type: 'input' | 'output' | 'error';
  text: string;
}

export function Terminal({ workspacePath, onClose, onStarted, initialCommand, sessionId: propSessionId }: TerminalProps) {
  const [lines, setLines] = useState<TermLine[]>([]);
  const [input, setInput] = useState('');
  const [started, setStarted] = useState(false);
  const [history, setHistory] = useState<string[]>([]);
  const [historyIdx, setHistoryIdx] = useState(-1);
  const [isFocused, setIsFocused] = useState(false);
  const outputRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const initialCommandSent = useRef(false);
  // Stop terminal only on true unmount — not on workspace change.
  useEffect(() => () => {
    typedInvoke<void>('tool_terminal_stop', { sessionId: stableIdRef.current }).catch(() => {});
  }, []);

  // Stable session id — generated once per mount, NOT per workspace change.
  // Rust tool_terminal_start already handles idempotent restart (kills old
  // entry with same id and spawns a new shell). Reusing the id means the
  // input handler always targets the correct session regardless of timing.
  const stableIdRef = useRef(propSessionId ?? `term-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`);
  const sessionIdRef = useRef(stableIdRef.current);

  useEffect(() => {
    const sessionId = stableIdRef.current;
    sessionIdRef.current = sessionId;
    let cancelled = false;
    setStarted(false);
    typedInvoke<string>('tool_terminal_start', { sessionId, cwd: workspacePath })
      .then(() => { if (!cancelled) setStarted(true); })
      .catch((e) => {
        if (!cancelled) {
          setLines([{ type: 'error', text: `Failed to start terminal: ${normalizeError(e).message}` }]);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [workspacePath]);

  // Send initial command after terminal starts
  useEffect(() => {
    if (started && initialCommand && !initialCommandSent.current) {
      initialCommandSent.current = true;
      typedInvoke<string>('tool_terminal_send', { sessionId: sessionIdRef.current, input: initialCommand })
        .then((output) => {
          if (output) {
            setLines((prev) => [...prev, { type: 'output', text: stripAnsi(output) }]);
          }
          onStarted?.();
        })
        .catch((e) => {
          setLines((prev) => [...prev, { type: 'error', text: normalizeError(e).message }]);
          onStarted?.();
        });
    }
  }, [started, initialCommand, onStarted]);

  useEffect(() => {
    if (outputRef.current) {
      outputRef.current.scrollTop = outputRef.current.scrollHeight;
    }
  }, [lines]);

  useEffect(() => {
    inputRef.current?.focus();
  }, [started]);

  const handleSubmit = useCallback(async () => {
    const cmd = input.trim();
    const sid = sessionIdRef.current;
    if (!cmd || !started || !sid) return;

    setInput('');
    setLines((prev) => [...prev, { type: 'input', text: cmd }]);
    setHistory((prev) => [...prev, cmd]);
    setHistoryIdx(-1);

    if (cmd === 'clear') {
      setLines([]);
      return;
    }

    try {
      const output = await typedInvoke<string>('tool_terminal_send', { sessionId: sessionIdRef.current, input: cmd });
      if (output) {
        setLines((prev) => [...prev, { type: 'output', text: stripAnsi(output) }]);
      }

      const { publishEvent } = useFileSyncStore.getState();
      const lowerCmd = cmd.toLowerCase();
      const writePatterns = [
        /^(echo|cat|tee|cp|mv|rm|touch|mkdir|chmod|chown)\s+/,
        /^(git\s+(add|rm|mv|checkout|switch|merge|rebase|commit))\s+/,
        /^(npm|yarn|pnpm|bun)\s+(init|install|uninstall)/,
        /^(pip|pip3)\s+(install|uninstall)/,
        /^(cargo)\s+(new|init)/,
      ];

      if (writePatterns.some((p) => p.test(lowerCmd))) {
        const parts = cmd.split(/\s+/);
        const fileArg = parts[parts.length - 1];
        if (fileArg && !fileArg.startsWith('-') && !fileArg.startsWith('|')) {
          await publishEvent(fileArg, 'modified', 'terminal');
        }
      }
    } catch (e) {
      setLines((prev) => [...prev, { type: 'error', text: normalizeError(e).message }]);
    }
  }, [input, started]);

  const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      handleSubmit();
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      if (history.length > 0) {
        const newIdx = historyIdx < history.length - 1 ? historyIdx + 1 : historyIdx;
        setHistoryIdx(newIdx);
        setInput(history[history.length - 1 - newIdx]);
      }
    } else if (e.key === 'ArrowDown') {
      e.preventDefault();
      if (historyIdx > 0) {
        const newIdx = historyIdx - 1;
        setHistoryIdx(newIdx);
        setInput(history[history.length - 1 - newIdx]);
      } else {
        setHistoryIdx(-1);
        setInput('');
      }
    } else if (e.key === 'l' && e.ctrlKey) {
      e.preventDefault();
      setLines([]);
    }
  }, [handleSubmit, history, historyIdx]);

  const cwdDisplay = workspacePath.split(/[\\/]/).filter(Boolean).pop() ?? '~';

  return (
    <div className={`${shared.terminalPanel}${isFocused ? '' : ' ' + shared.terminalPanelBlurred}`}>
      <div className={shared.terminalPanelHeader}>
        <span className={shared.terminalPanelTitle}>
          <svg viewBox="0 0 16 16" width="13" height="13" fill="none" aria-hidden="true">
            <rect x="1.5" y="2.5" width="13" height="11" rx="2" stroke="currentColor" strokeWidth="1.2" />
            <path d="M4 7l3 2.5L4 12" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" strokeLinejoin="round" />
            <path d="M8.5 12h4" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" />
          </svg>
          Terminal
        </span>
        <span className={shared.terminalPanelCwd}>{cwdDisplay}</span>
        <div className={shared.terminalPanelActions}>
          <button className={shared.terminalPanelBtn} onClick={() => setLines([])} title="Clear (Ctrl+L)">
            <svg viewBox="0 0 16 16" width="13" height="13" fill="none" aria-hidden="true">
              <path d="M2 4h12M5 4V3a1 1 0 011-1h4a1 1 0 011 1v1M6 7v4M8.5 7v4M11 7v4" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" />
            </svg>
          </button>
          <button className={shared.terminalPanelBtn} onClick={onClose} title="Close terminal">
            <svg viewBox="0 0 16 16" width="13" height="13" fill="none" aria-hidden="true">
              <path d="M4 4l8 8M12 4l-8 8" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" />
            </svg>
          </button>
        </div>
      </div>
      <div
        className={shared.terminalPanelOutput}
        ref={outputRef}
        onClick={() => { if (started) inputRef.current?.focus(); }}
      >
        {/* Hidden input captures all keystrokes */}
        <input
          ref={inputRef}
          className={shared.terminalPanelHiddenInput}
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={handleKeyDown}
          onFocus={() => setIsFocused(true)}
          onBlur={() => setIsFocused(false)}
          disabled={!started}
          spellCheck={false}
          autoComplete="off"
          aria-label="Terminal input"
        />
        {lines.length === 0 && history.length === 0 && (
          <div className={shared.terminalPanelWelcome}>
            Terminal session started. Click here and type commands.
          </div>
        )}
        {lines.map((line, i) => (
          <div key={i} className={`${shared.terminalPanelLine} ${shared[`terminalPanelLine--${line.type}`]}`}>
            {line.type === 'input' && <span className={shared.terminalPanelPrompt}>$ </span>}
            <pre className={shared.terminalPanelText}>{line.text}</pre>
          </div>
        ))}
        {/* Active prompt line — always visible at bottom */}
        <div className={`${shared.terminalPanelLine} ${shared['terminalPanelLine--active']}`}>
          <span className={shared.terminalPanelPrompt}>$ </span>
          <span className={shared.terminalPanelActiveText}>{input}</span>
          <span className={shared.terminalPanelCursor} />
        </div>
      </div>
    </div>
  );
}
