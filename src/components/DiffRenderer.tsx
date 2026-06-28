import { useMemo } from 'react';
import styles from './DiffRenderer.module.css';

// ---- Types ----

interface DiffHunk {
  oldStart: number;
  oldCount: number;
  newStart: number;
  newCount: number;
  lines: DiffLine[];
}

interface DiffLine {
  kind: 'add' | 'del' | 'context';
  oldLine?: number;
  newLine?: number;
  text: string;
}

interface DiffFile {
  header: string;
  oldFile: string;
  newFile: string;
  hunks: DiffHunk[];
}

interface DiffRendererProps {
  /** Raw unified diff text. */
  content: string;
}

// ---- Parser ----

/** Parse a unified diff string into structured DiffFile objects. */
function parseDiff(raw: string): DiffFile[] {
  const lines = raw.replace(/\r\n/g, '\n').split('\n');
  const files: DiffFile[] = [];
  let current: DiffFile | null = null;
  let currentHunk: DiffHunk | null = null;

  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];

    // File header: "diff --git a/X b/Y"
    const fileMatch = /^diff --git a\/(.+?) b\/(.+?)$/.exec(line);
    if (fileMatch) {
      if (current) files.push(current);
      current = { header: line, oldFile: fileMatch[1], newFile: fileMatch[2], hunks: [] };
      currentHunk = null;
      continue;
    }

    if (!current) continue;

    // Extended header lines (index, ---, +++, etc.) - attach to file header
    if (
      /^(index |--- |\+\+\+ )/.test(line) ||
      /^(old mode |new mode |deleted file |new file |rename |copy |similarity index )/.test(line) ||
      /^(Binary files )/.test(line) ||
      /^@@/.test(line) === false && currentHunk === null
    ) {
      // Collect extended headers only before first hunk
      if (currentHunk === null && !/^@@/.test(line)) {
        current.header += '\n' + line;
      }
    }

    // Hunk header: "@@ -l,s +l,s @@"
    const hunkMatch = /^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@(.*)$/.exec(line);
    if (hunkMatch) {
      currentHunk = {
        oldStart: Number(hunkMatch[1]),
        oldCount: hunkMatch[2] !== undefined ? Number(hunkMatch[2]) : 1,
        newStart: Number(hunkMatch[3]),
        newCount: hunkMatch[4] !== undefined ? Number(hunkMatch[4]) : 1,
        lines: [],
      };
      current.hunks.push(currentHunk);
      continue;
    }

    if (!currentHunk) continue;

    // Diff lines within a hunk
    let oldLine = currentHunk.oldStart;
    let newLine = currentHunk.newStart;

    // Recalculate based on existing lines
    for (const l of currentHunk.lines) {
      if (l.kind !== 'add') oldLine++;
      if (l.kind !== 'del') newLine++;
    }

    if (line.startsWith('+')) {
      currentHunk.lines.push({ kind: 'add', newLine, text: line.slice(1) });
    } else if (line.startsWith('-')) {
      currentHunk.lines.push({ kind: 'del', oldLine, text: line.slice(1) });
    } else if (line.startsWith('\\ ')) {
      // No newline at end of file marker - skip silently
      continue;
    } else {
      currentHunk.lines.push({ kind: 'context', oldLine, newLine, text: line.startsWith(' ') ? line.slice(1) : line });
    }
  }

  if (current) files.push(current);
  return files;
}

// ---- Component ----

/**
 * Renders a unified diff with syntax coloring and line numbers.
 *
 * Features:
 * - File headers with old/new path display
 * - Hunk headers with line ranges
 * - Added lines in green, removed lines in red
 * - Line numbers for both old and new files
 * - Context lines with muted styling
 */
export function DiffRenderer({ content }: DiffRendererProps) {
  const files = useMemo(() => parseDiff(content), [content]);

  if (files.length === 0) {
    return <pre className="diff-block">{content}</pre>;
  }

  return (
    <div className={styles.viewer}>
      {files.map((file, fi) => (
        <div key={fi} className={styles.file}>
          <DiffFileHeader file={file} />
          <div className={styles.fileBody}>
            {file.hunks.map((hunk, hi) => (
              <DiffHunkView key={hi} hunk={hunk} />
            ))}
          </div>
        </div>
      ))}
    </div>
  );
}

/** File-level header showing the diff command and old/new paths. */
function DiffFileHeader({ file }: { file: DiffFile }) {
  return (
    <div className={styles.fileHeader}>
      <div className={styles.fileHeaderMain}>{file.header.split('\n')[0]}</div>
      {file.oldFile !== file.newFile && (
        <div className={styles.fileHeaderPaths}>
          <span className={`${styles.path} ${styles['path--old']}`}>
            {file.oldFile}
          </span>
          <span className={styles.arrow}>&rarr;</span>
          <span className={`${styles.path} ${styles['path--new']}`}>
            {file.newFile}
          </span>
        </div>
      )}
    </div>
  );
}

/** A single hunk (group of changes) in a diff file. */
function DiffHunkView({ hunk }: { hunk: DiffHunk }) {
  return (
    <div className="diff-hunk">
      <div className={styles.hunkHeader}>
        @@ -{hunk.oldStart},{hunk.oldCount} +{hunk.newStart},{hunk.newCount} @@
      </div>
      {hunk.lines.map((line, li) => (
        <DiffLineView key={li} line={line} />
      ))}
    </div>
  );
}

/** A single line within a diff hunk. */
function DiffLineView({ line }: { line: DiffLine }) {
  return (
    <div className={`${styles.line} ${styles[`line--${line.kind}`]}`}>
      <span className={styles.sign}>
        {line.kind === 'add' ? '+' : line.kind === 'del' ? '-' : ' '}
      </span>
      <span className={styles.oldNum}>
        {line.oldLine !== undefined ? line.oldLine : ''}
      </span>
      <span className={styles.newNum}>
        {line.newLine !== undefined ? line.newLine : ''}
      </span>
      <span className={styles.text}>{line.text}</span>
    </div>
  );
}
