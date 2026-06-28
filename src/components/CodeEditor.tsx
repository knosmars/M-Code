import hljs from 'highlight.js';
import shared from './CodeEditor.module.css';

interface CodeEditorProps {
  /** Absolute or relative file path. */
  filePath: string;
  /** Pre-fetched file content. */
  content: string | null;
}

/** Map file extension → highlight.js language identifier. */
function languageFromPath(path: string): string {
  const ext = path.split('.').pop()?.toLowerCase();
  switch (ext) {
    case 'ts':
    case 'tsx':
      return 'typescript';
    case 'js':
    case 'jsx':
      return 'javascript';
    case 'rs':
      return 'rust';
    case 'py':
      return 'python';
    case 'go':
      return 'go';
    case 'java':
      return 'java';
    case 'rb':
      return 'ruby';
    case 'css':
      return 'css';
    case 'scss':
      return 'scss';
    case 'html':
      return 'xml';
    case 'json':
      return 'json';
    case 'yaml':
    case 'yml':
      return 'yaml';
    case 'md':
      return 'markdown';
    case 'sh':
    case 'bash':
      return 'bash';
    case 'sql':
      return 'sql';
    case 'toml':
      return 'ini';
    case 'xml':
      return 'xml';
    case 'c':
    case 'h':
      return 'c';
    case 'cpp':
    case 'hpp':
    case 'cxx':
      return 'cpp';
    default:
      return 'plaintext';
  }
}

/** Read-only code editor with syntax highlighting and line numbers. */
export function CodeEditor({ filePath, content }: CodeEditorProps) {
  if (content === null) {
    return (
      <div className={shared.codeEditor}>
        <div className={shared.codeEditorLoading}>Loading…</div>
      </div>
    );
  }

  const lang = languageFromPath(filePath);
  const highlighted: string = (() => {
    try {
      const result = hljs.highlight(content, { language: lang });
      return result.value;
    } catch {
      return hljs.highlightAuto(content).value;
    }
  })();

  const lines = content.split('\n');
  const lineCount = lines.length;
  const gutterWidth = 8 + String(lineCount).length * 10;

  return (
    <div className={shared.codeEditor}>
      <div className={shared.codeEditorHeader}>
        <span className={shared.codeEditorTitle}>{filePath.split('/').pop() ?? filePath}</span>
        <span className={shared.codeEditorLang}>{lang}</span>
        <span className={shared.codeEditorStats}>{lineCount} lines</span>
      </div>
      <div className={shared.codeEditorBody}>
        <div className={shared.codeEditorGutter} style={{ minWidth: `${gutterWidth}px` }}>
          {lines.map((_, i) => (
            <div key={i} className={shared.codeEditorLineNumber}>
              {i + 1}
            </div>
          ))}
        </div>
        <pre className={shared.codeEditorContent}>
          <code
            className={`${shared.codeEditorCode} hljs language-${lang}`}
            dangerouslySetInnerHTML={{ __html: highlighted }}
          />
        </pre>
      </div>
    </div>
  );
}
