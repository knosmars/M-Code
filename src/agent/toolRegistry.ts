import type { ToolDefinition } from './tools';
import { typedInvoke } from '../utils/ipc';
import { useFileSyncStore } from '../stores/fileSyncStore';

export interface ToolSpec {
  definition: ToolDefinition;
  sideEffect: boolean;
  invoke: (args: Record<string, unknown>) => Promise<string>;
}

const cmd =
  (command: string, mapArgs: (a: Record<string, unknown>) => Record<string, unknown>) =>
  (args: Record<string, unknown>): Promise<string> =>
    typedInvoke<string>(command, mapArgs(args));

export const TOOL_REGISTRY: ToolSpec[] = [
  // ── read_file ──
  {
    definition: {
      name: 'read_file',
      description:
        'Read the entire contents of a file. Returns the file content as text, or an error if the file does not exist.',
      parameters: {
        type: 'object',
        properties: {
          path: {
            type: 'string',
            description: 'Absolute or relative path to the file to read.',
          },
        },
        required: ['path'],
      },
    },
    sideEffect: false,
    invoke: async (a) => {
      await useFileSyncStore.getState().registerInterest('chat', a.path as string);
      return typedInvoke<string>('tool_read_file', { path: a.path });
    },
  },

  // ── write_file ──
  {
    definition: {
      name: 'write_file',
      description:
        'Write content to a file, creating it if it does not exist or overwriting if it does. Use with caution.',
      parameters: {
        type: 'object',
        properties: {
          path: {
            type: 'string',
            description: 'Absolute or relative path to the file to write.',
          },
          content: {
            type: 'string',
            description: 'The content to write to the file.',
          },
        },
        required: ['path', 'content'],
      },
    },
    sideEffect: true,
    invoke: async (a) => {
      await useFileSyncStore.getState().registerInterest('chat', a.path as string);
      await useFileSyncStore.getState().publishEvent(a.path as string, 'modified', 'chat');
      return typedInvoke<string>('tool_write_file', { path: a.path, content: a.content });
    },
  },

  // ── edit_file ──
  {
    definition: {
      name: 'edit_file',
      description:
        'Make a targeted edit to a file by replacing one string with another. Only the first match is replaced. Use this for surgical changes instead of write_file when possible.',
      parameters: {
        type: 'object',
        properties: {
          path: {
            type: 'string',
            description: 'Path to the file to edit.',
          },
          oldString: {
            type: 'string',
            description: 'The exact text to replace.',
          },
          newString: {
            type: 'string',
            description: 'The text to replace it with.',
          },
        },
        required: ['path', 'oldString', 'newString'],
      },
    },
    sideEffect: true,
    invoke: async (a) => {
      await useFileSyncStore.getState().registerInterest('chat', a.path as string);
      await useFileSyncStore.getState().publishEvent(a.path as string, 'modified', 'chat');
      return typedInvoke<string>('tool_edit_file', {
        path: a.path,
        oldString: a.oldString,
        newString: a.newString,
      });
    },
  },

  // ── edit_file_preview ──
  {
    definition: {
      name: 'edit_file_preview',
      description: 'Preview the diff that edit_file would produce WITHOUT applying changes. Takes the same parameters as edit_file (path, old_string, new_string) and returns a unified diff string. Use this to show the user what would change before committing.',
      parameters: {
        type: 'object',
        properties: {
          path: { type: 'string', description: 'Absolute path to the file to preview changes for' },
          old_string: { type: 'string', description: 'The exact text to find in the file' },
          new_string: { type: 'string', description: 'The text to replace it with' },
        },
        required: ['path', 'old_string', 'new_string'],
        additionalProperties: false,
      },
    },
    sideEffect: false,
    invoke: cmd('tool_edit_file_preview', (a) => ({
      path: a.path,
      old_string: a.old_string,
      new_string: a.new_string,
    })),
  },

  // ── multi_edit_preview ──
  {
    definition: {
      name: 'multi_edit_preview',
      description: 'Preview combined diff for multiple file edits WITHOUT applying. Takes an array of edit operations (each with path, old_string, new_string) and returns a unified diff showing all changes. Use to review multi-file changes before committing.',
      parameters: {
        type: 'object',
        properties: {
          edits: {
            type: 'array',
            description: 'Array of edit operations to preview',
            items: {
              type: 'object',
              properties: {
                path: { type: 'string', description: 'Absolute path to the file' },
                old_string: { type: 'string', description: 'Exact text to find' },
                new_string: { type: 'string', description: 'Text to replace with' },
              },
              required: ['path', 'old_string', 'new_string'],
              additionalProperties: false,
            },
          },
        },
        required: ['edits'],
        additionalProperties: false,
      },
    },
    sideEffect: false,
    invoke: cmd('tool_multi_edit_preview', (a) => ({ edits: a.edits })),
  },

  // ── multi_edit_apply ──
  {
    definition: {
      name: 'multi_edit_apply',
      description: 'Apply multiple file edits atomically — all succeed or all fail. Takes an array of edit operations. Validates all edits first, then applies them. If any edit fails validation, no files are modified.',
      parameters: {
        type: 'object',
        properties: {
          edits: {
            type: 'array',
            description: 'Array of edit operations to apply',
            items: {
              type: 'object',
              properties: {
                path: { type: 'string', description: 'Absolute path to the file' },
                old_string: { type: 'string', description: 'Exact text to find' },
                new_string: { type: 'string', description: 'Text to replace with' },
              },
              required: ['path', 'old_string', 'new_string'],
              additionalProperties: false,
            },
          },
        },
        required: ['edits'],
        additionalProperties: false,
      },
    },
    sideEffect: false,
    invoke: cmd('tool_multi_edit_apply', (a) => ({ edits: a.edits })),
  },

  // ── list_dir ──
  {
    definition: {
      name: 'list_dir',
      description:
        'List the contents of a directory. Returns an array of file and directory names.',
      parameters: {
        type: 'object',
        properties: {
          path: {
            type: 'string',
            description: 'Path to the directory to list.',
          },
        },
        required: ['path'],
      },
    },
    sideEffect: false,
    invoke: cmd('tool_list_dir', (a) => ({ path: a.path })),
  },

  // ── terminal_start ──
  {
    definition: {
      name: 'terminal_start',
      description: 'Start a persistent shell session for interactive commands',
      parameters: {
        type: 'object',
        properties: {
          session_id: { type: 'string', description: 'Unique session identifier' },
          cwd: { type: 'string', description: 'Working directory' },
        },
        required: ['session_id'],
      },
    },
    sideEffect: true,
    invoke: cmd('tool_terminal_start', (a) => ({ session_id: a.session_id, cwd: a.cwd })),
  },

  // ── terminal_send ──
  {
    definition: {
      name: 'terminal_send',
      description: 'Send input to a terminal session and read output',
      parameters: {
        type: 'object',
        properties: {
          session_id: { type: 'string' },
          input: { type: 'string', description: 'Command or input to send' },
        },
        required: ['session_id', 'input'],
      },
    },
    sideEffect: true,
    invoke: cmd('tool_terminal_send', (a) => ({ session_id: a.session_id, input: a.input })),
  },

  // ── terminal_stop ──
  {
    definition: {
      name: 'terminal_stop',
      description: 'Stop and clean up a terminal session',
      parameters: {
        type: 'object',
        properties: {
          session_id: { type: 'string' },
        },
        required: ['session_id'],
      },
    },
    sideEffect: true,
    invoke: cmd('tool_terminal_stop', (a) => ({ session_id: a.session_id })),
  },

  // ── terminal_list ──
  {
    definition: {
      name: 'terminal_list',
      description: 'List all active terminal sessions',
      parameters: { type: 'object', properties: {} },
    },
    sideEffect: false,
    invoke: cmd('tool_terminal_list', () => ({})),
  },

  // ── run_command ──
  {
    definition: {
      name: 'run_command',
      description:
        'Execute a shell command in the workspace directory and return stdout + stderr. Requires user permission.',
      parameters: {
        type: 'object',
        properties: {
          command: {
            type: 'string',
            description: 'The shell command to execute.',
          },
          workdir: {
            type: 'string',
            description:
              'Optional working directory for the command. Defaults to the workspace root.',
          },
        },
        required: ['command'],
      },
    },
    sideEffect: true,
    invoke: async (a) => {
      const cwd = a.workdir !== undefined ? a.workdir : undefined;
      const params: Record<string, unknown> = { command: a.command };
      if (cwd !== undefined) {
        params.cwd = cwd;
      }
      const raw = await typedInvoke<string>('tool_run_command', params);
      const exitMatch = raw.match(/\[exit code: \d+\]/);
      const stderrMatch = raw.match(/\[stderr\]\n([\s\S]*)$/);
      if (exitMatch) {
        let stderr = (stderrMatch?.[1] || '').trim();
        const uselessPatterns = [
          /SyntaxError:/i,
          /underterminated string literal/i,
          /is not recognized as an internal or external command/i,
          /'bc' is not recognized/i,
          /File "<string>":/i,
        ];
        for (const p of uselessPatterns) {
          stderr = stderr.replace(p, '');
        }
        stderr = stderr.trim();
        return stderr || 'Command failed';
      }
      return raw;
    },
  },

  // ── grep ──
  {
    definition: {
      name: 'grep',
      description:
        'Search file contents using a regular expression and return matching lines.',
      parameters: {
        type: 'object',
        properties: {
          pattern: {
            type: 'string',
            description: 'The regex pattern to search for.',
          },
          path: {
            type: 'string',
            description: 'Optional directory to search in. Defaults to the workspace root.',
          },
          include: {
            type: 'string',
            description: 'Optional file pattern filter (e.g. "*.ts").',
          },
        },
        required: ['pattern'],
      },
    },
    sideEffect: false,
    invoke: (a) =>
      typedInvoke<string>('tool_grep', {
        pattern: a.pattern,
        path: a.path ?? '.',
        include: a.include,
      }),
  },

  // ── glob ──
  {
    definition: {
      name: 'glob',
      description:
        'Find files matching a glob pattern and return their paths.',
      parameters: {
        type: 'object',
        properties: {
          pattern: {
            type: 'string',
            description: 'The glob pattern to match (e.g. "**/*.ts").',
          },
          path: {
            type: 'string',
            description: 'Optional base directory for the search. Defaults to the workspace root.',
          },
        },
        required: ['pattern'],
      },
    },
    sideEffect: false,
    invoke: (a) => {
      const params: Record<string, unknown> = { pattern: a.pattern };
      if (a.path !== undefined) {
        params.path = a.path;
      }
      return typedInvoke<string>('tool_glob', params);
    },
  },

  // ── search_codebase ──
  {
    definition: {
      name: 'search_codebase',
      description:
        'Search the codebase for a symbol, function, class, type, or pattern. Returns matching locations with context, classified as definitions, imports, or references. More targeted than grep — understands code structure. Use this to find where something is defined, imported, or used.',
      parameters: {
        type: 'object',
        properties: {
          query: {
            type: 'string',
            description: 'The symbol name, function name, class name, or pattern to search for.',
          },
          path: {
            type: 'string',
            description: 'Root directory to search in. Use "." for the entire workspace.',
          },
        },
        required: ['query', 'path'],
      },
    },
    sideEffect: false,
    invoke: (a) =>
      typedInvoke<string>('tool_search_codebase', {
        query: a.query,
        path: a.path ?? '.',
      }),
  },

  // ── semantic_search ──
  {
    definition: {
      name: 'semantic_search',
      description:
        'Find code by meaning using embeddings (not exact text). Best for "where is the logic that does X", "code related to Y", or locating relevant areas when you do not know the exact name. For exact symbol/string matches prefer search_codebase or grep. Requires the workspace to have been indexed (semantic_index); returns top matches with path, line range, score, and snippet.',
      parameters: {
        type: 'object',
        properties: {
          query: { type: 'string', description: 'Natural-language description of the code you are looking for.' },
          top_k: { type: 'number', description: 'How many results to return (default 5).' },
        },
        required: ['query'],
      },
    },
    sideEffect: false,
    invoke: (a) =>
      typedInvoke<string>('tool_semantic_search', {
        query: a.query,
        path: '.',
        topK: a.top_k,
      }),
  },

  // ── semantic_index ──
  {
    definition: {
      name: 'semantic_index',
      description:
        'Build or refresh the semantic search index for the workspace (embeds source files). Run once before semantic_search, or after large changes. Incremental — unchanged files are skipped.',
      parameters: { type: 'object', properties: {} },
    },
    sideEffect: false,
    invoke: () => typedInvoke<string>('tool_semantic_index', { path: '.' }),
  },

  // ── generate_image ──
  {
    definition: {
      name: 'generate_image',
      description:
        'Generate an image from a text prompt using AI. Returns a base64 data URL of the generated image. Use for creating illustrations, diagrams, or visual content.',
      parameters: {
        type: 'object',
        properties: {
          prompt: {
            type: 'string',
            description: 'Detailed description of the image to generate',
          },
          size: {
            type: 'string',
            description: 'Image size: 1024x1024, 1792x1024, or 1024x1792. Default: 1024x1024',
          },
          style: {
            type: 'string',
            description: 'Image style: natural or vivid. Default: natural',
          },
        },
        required: ['prompt'],
      },
    },
    sideEffect: true,
    invoke: (a) => {
      const params: Record<string, unknown> = { prompt: a.prompt };
      if (a.size !== undefined) params.size = a.size;
      if (a.style !== undefined) params.style = a.style;
      return typedInvoke<string>('tool_generate_image', params);
    },
  },

  // ── lsp_hover ──
  {
    definition: {
      name: 'lsp_hover',
      description:
        'Get hover information for a symbol at a specific position (type, signature, documentation). Works like an LSP hover request but uses text-based analysis. Supports TypeScript, JavaScript, Rust, and Python.',
      parameters: {
        type: 'object',
        properties: {
          path: {
            type: 'string',
            description: 'Absolute path to the source file.',
          },
          line: {
            type: 'integer',
            description: '0-based line number.',
          },
          column: {
            type: 'integer',
            description: '0-based column (character) index.',
          },
        },
        required: ['path', 'line', 'column'],
        additionalProperties: false,
      },
    },
    sideEffect: false,
    invoke: cmd('tool_lsp_hover', (a) => ({
      path: a.path,
      line: a.line,
      column: a.column,
    })),
  },

  // ── lsp_go_to_definition ──
  {
    definition: {
      name: 'lsp_go_to_definition',
      description:
        'Find the definition location of a symbol at a specific position. Returns the file path, line, column, and surrounding context. Supports TypeScript, JavaScript, Rust, and Python.',
      parameters: {
        type: 'object',
        properties: {
          path: {
            type: 'string',
            description: 'Absolute path to the source file.',
          },
          line: {
            type: 'integer',
            description: '0-based line number.',
          },
          column: {
            type: 'integer',
            description: '0-based column (character) index.',
          },
        },
        required: ['path', 'line', 'column'],
        additionalProperties: false,
      },
    },
    sideEffect: false,
    invoke: cmd('tool_lsp_go_to_definition', (a) => ({
      path: a.path,
      line: a.line,
      column: a.column,
    })),
  },

  // ── lsp_find_references ──
  {
    definition: {
      name: 'lsp_find_references',
      description:
        'Find all references to a symbol at a specific position across the workspace. Returns up to 100 references with file path, line, column, and context. Supports TypeScript, JavaScript, Rust, and Python.',
      parameters: {
        type: 'object',
        properties: {
          path: {
            type: 'string',
            description: 'Absolute path to the source file.',
          },
          line: {
            type: 'integer',
            description: '0-based line number.',
          },
          column: {
            type: 'integer',
            description: '0-based column (character) index.',
          },
          include_definition: {
            type: 'boolean',
            description: 'Whether to include the definition site in results (default: true).',
          },
        },
        required: ['path', 'line', 'column'],
        additionalProperties: false,
      },
    },
    sideEffect: false,
    invoke: (a) => {
      const params: Record<string, unknown> = {
        path: a.path,
        line: a.line,
        column: a.column,
      };
      if (a.include_definition !== undefined) {
        params.include_definition = a.include_definition;
      }
      return typedInvoke<string>('tool_lsp_find_references', params);
    },
  },

  // ── git_status ──
  {
    definition: {
      name: 'git_status',
      description:
        'Show the working tree status in short format, including current branch.',
      parameters: {
        type: 'object',
        properties: {
          path: {
            type: 'string',
            description: 'Workspace directory path.',
          },
        },
        required: ['path'],
      },
    },
    sideEffect: false,
    invoke: cmd('tool_git_status', (a) => ({ path: a.path })),
  },

  // ── git_diff ──
  {
    definition: {
      name: 'git_diff',
      description:
        'Show unstaged changes in the working tree as a unified diff.',
      parameters: {
        type: 'object',
        properties: {
          path: {
            type: 'string',
            description: 'Workspace directory path.',
          },
        },
        required: ['path'],
      },
    },
    sideEffect: false,
    invoke: cmd('tool_git_diff', (a) => ({ path: a.path })),
  },

  // ── git_diff_staged ──
  {
    definition: {
      name: 'git_diff_staged',
      description:
        'Show staged changes (ready to commit) as a unified diff.',
      parameters: {
        type: 'object',
        properties: {
          path: {
            type: 'string',
            description: 'Workspace directory path.',
          },
        },
        required: ['path'],
      },
    },
    sideEffect: false,
    invoke: cmd('tool_git_diff_staged', (a) => ({ path: a.path })),
  },

  // ── git_log ──
  {
    definition: {
      name: 'git_log',
      description:
        'Show recent commit history in one-line format with decorations.',
      parameters: {
        type: 'object',
        properties: {
          path: {
            type: 'string',
            description: 'Workspace directory path.',
          },
          n: {
            type: 'integer',
            description: 'Number of commits to show (default 10).',
          },
        },
        required: ['path'],
      },
    },
    sideEffect: false,
    invoke: cmd('tool_git_log', (a) => ({ path: a.path, n: a.n })),
  },

  // ── git_commit ──
  {
    definition: {
      name: 'git_commit',
      description:
        'Stage and commit changes. Use run_command with git add first to stage files. Requires a meaningful commit message.',
      parameters: {
        type: 'object',
        properties: {
          path: {
            type: 'string',
            description: 'Workspace directory path.',
          },
          message: {
            type: 'string',
            description: 'Commit message describing the changes.',
          },
        },
        required: ['path', 'message'],
      },
    },
    sideEffect: true,
    invoke: cmd('tool_git_commit', (a) => ({ path: a.path, message: a.message })),
  },

  // ── git_branch ──
  {
    definition: {
      name: 'git_branch',
      description:
        'Switch to an existing branch or create and switch to a new one.',
      parameters: {
        type: 'object',
        properties: {
          path: {
            type: 'string',
            description: 'Workspace directory path.',
          },
          name: {
            type: 'string',
            description: 'Branch name to switch to or create.',
          },
          create: {
            type: 'boolean',
            description: 'If true, create the branch before switching.',
          },
        },
        required: ['path', 'name'],
      },
    },
    sideEffect: true,
    invoke: (a) => {
      const params: Record<string, unknown> = { path: a.path, name: a.name };
      if (a.create !== undefined) {
        params.create = a.create;
      }
      return typedInvoke<string>('tool_git_branch', params);
    },
  },

  // ── git_push ──
  {
    definition: {
      name: 'git_push',
      description:
        'Push the current branch to origin. Use after committing changes.',
      parameters: {
        type: 'object',
        properties: {
          path: {
            type: 'string',
            description: 'Workspace directory path.',
          },
        },
        required: ['path'],
      },
    },
    sideEffect: true,
    invoke: cmd('tool_git_push', (a) => ({ path: a.path })),
  },

  // ── gh_pr_create ──
  {
    definition: {
      name: 'gh_pr_create',
      description:
        'Create a GitHub pull request from the current branch. Requires gh CLI to be installed and authenticated. Returns the PR URL.',
      parameters: {
        type: 'object',
        properties: {
          path: {
            type: 'string',
            description: 'Workspace directory path.',
          },
          title: {
            type: 'string',
            description: 'PR title.',
          },
          body: {
            type: 'string',
            description: 'Optional PR description.',
          },
          base: {
            type: 'string',
            description: 'Optional base branch (default: main).',
          },
        },
        required: ['path', 'title'],
      },
    },
    sideEffect: true,
    invoke: (a) => {
      const params: Record<string, unknown> = { path: a.path, title: a.title };
      if (a.body !== undefined) params.body = a.body;
      if (a.base !== undefined) params.base = a.base;
      return typedInvoke<string>('tool_gh_pr_create', params);
    },
  },

  // ── memory_read ──
  {
    definition: {
      name: 'memory_read',
      description:
        'Read the project memory file (.meyatu/memory.md). Contains conventions, preferences, and context that persist across sessions.',
      parameters: {
        type: 'object',
        properties: {
          path: {
            type: 'string',
            description: 'Workspace directory path.',
          },
        },
        required: ['path'],
      },
    },
    sideEffect: false,
    invoke: cmd('tool_memory_read', (a) => ({ path: a.path })),
  },

  // ── memory_write ──
  {
    definition: {
      name: 'memory_write',
      description:
        'Write to the project memory file (.meyatu/memory.md). Use it to persist durable learnings across sessions: a project convention, the user\'s stated preference, a non-obvious gotcha, or the fix/lesson from a problem that could recur. Keep entries short and specific. This is how you improve over time — record a lesson when you learn it rather than losing it at session end. When memory grows redundant, read it, then rewrite it with mode "replace" to merge duplicates, drop the obsolete, and regroup under headings (e.g. Conventions / Gotchas / User preferences).',
      parameters: {
        type: 'object',
        properties: {
          path: {
            type: 'string',
            description: 'Workspace directory path.',
          },
          content: {
            type: 'string',
            description: 'The note to record (append mode), or the full curated memory (replace mode).',
          },
          mode: {
            type: 'string',
            enum: ['append', 'replace'],
            description: 'append (default): add a timestamped note. replace: overwrite the whole memory file with your curated content — use it to dedup/reorganize.',
          },
        },
        required: ['path', 'content'],
      },
    },
    sideEffect: false,
    invoke: cmd('tool_memory_write', (a) => ({
      path: a.path,
      content: a.content,
      mode: a.mode,
    })),
  },

  // ── memory_search ──
  {
    definition: {
      name: 'memory_search',
      description: 'Search project memory for relevant conventions and notes using keyword matching',
      parameters: {
        type: 'object',
        properties: {
          path: { type: 'string', description: 'Workspace path' },
          query: { type: 'string', description: 'Search query' },
          limit: { type: 'number', description: 'Max results (default 5)' },
        },
        required: ['path', 'query'],
      },
    },
    sideEffect: false,
    invoke: cmd('tool_memory_search', (a) => ({
      path: a.path,
      query: a.query,
      limit: a.limit,
    })),
  },

  // ── agents_rules_read ──
  {
    definition: {
      name: 'agents_rules_read',
      description:
        'Read the project rules file (.meyatu/agents.yml). Returns structured coding rules (strict, suggest, policy) and a generated system prompt. Use this to understand project conventions and constraints.',
      parameters: {
        type: 'object',
        properties: {
          path: {
            type: 'string',
            description: 'Workspace directory path.',
          },
        },
        required: ['path'],
      },
    },
    sideEffect: false,
    invoke: cmd('tool_agents_rules_read', (a) => ({ path: a.path })),
  },

  // ── hooks_run ──
  {
    definition: {
      name: 'hooks_run',
      description:
        'Run lifecycle hooks (shell commands) for a given event. Hooks are configured in .meyatu/hooks.json. Events: before_tool, after_tool, before_chat, after_chat. Returns a list of hook results with exit codes, stdout/stderr, and whether execution was blocked.',
      parameters: {
        type: 'object',
        properties: {
          path: {
            type: 'string',
            description: 'Workspace directory path.',
          },
          event: {
            type: 'string',
            description: 'Hook event: before_tool, after_tool, before_chat, or after_chat.',
          },
          tool_name: {
            type: 'string',
            description: 'Tool name for before_tool/after_tool events.',
          },
          tool_args: {
            type: 'string',
            description: 'JSON string of tool arguments.',
          },
          tool_result: {
            type: 'string',
            description: 'Tool result output for after_tool events.',
          },
          tool_error: {
            type: 'string',
            description: 'Tool error message if the tool failed.',
          },
          session_id: {
            type: 'string',
            description: 'Current session ID.',
          },
        },
        required: ['path', 'event'],
      },
    },
    sideEffect: true,
    invoke: (a) => {
      const params: Record<string, unknown> = {
        path: a.path,
        event: a.event,
        toolName: a.tool_name,
        toolArgs: a.tool_args,
        toolResult: a.tool_result,
        toolError: a.tool_error,
        sessionId: a.session_id,
      };
      return typedInvoke<string>('tool_hooks_run', params);
    },
  },

  // ── triggers_list ──
  {
    definition: {
      name: 'triggers_list',
      description:
        'List all triggers configured in .meyatu/triggers.yml. Returns trigger definitions (file_watch, schedule, webhook) and count of active auto-run triggers.',
      parameters: {
        type: 'object',
        properties: {
          path: {
            type: 'string',
            description: 'Workspace directory path.',
          },
        },
        required: ['path'],
      },
    },
    sideEffect: false,
    invoke: cmd('tool_triggers_list', (a) => ({ path: a.path })),
  },

  // ── triggers_watch ──
  {
    definition: {
      name: 'triggers_watch',
      description:
        'Start a single trigger (file_watch, schedule, or webhook) in the background by id. The trigger runs until the app exits. Specify the trigger id from triggers.yml.',
      parameters: {
        type: 'object',
        properties: {
          path: {
            type: 'string',
            description: 'Workspace directory path.',
          },
          trigger_id: {
            type: 'string',
            description: 'Trigger id from triggers.yml.',
          },
        },
        required: ['path', 'trigger_id'],
      },
    },
    sideEffect: true,
    invoke: cmd('tool_triggers_watch', (a) => ({
      path: a.path,
      triggerId: a.trigger_id,
    })),
  },

  // ── triggers_start_auto ──
  {
    definition: {
      name: 'triggers_start_auto',
      description:
        'Start every trigger in triggers.yml whose auto_run flag is set. Use this once a workspace is opened to bring up background watchers/schedules/webhooks without starting each one individually. Returns a summary of which triggers started or failed.',
      parameters: {
        type: 'object',
        properties: {
          path: {
            type: 'string',
            description: 'Workspace directory path.',
          },
        },
        required: ['path'],
      },
    },
    sideEffect: true,
    invoke: cmd('tool_triggers_start_auto', (a) => ({ path: a.path })),
  },

  // ── skills_list ──
  {
    definition: {
      name: 'skills_list',
      description:
        'List all skills from .meyatu/skills/ directory (YAML files). Each skill provides a name, description, and category.',
      parameters: {
        type: 'object',
        properties: {
          path: {
            type: 'string',
            description: 'Workspace directory path.',
          },
        },
        required: ['path'],
      },
    },
    sideEffect: false,
    invoke: cmd('tool_skills_list', (a) => ({ path: a.path })),
  },

  // ── skills_load ──
  {
    definition: {
      name: 'skills_load',
      description:
        'Load a specific skill by name from .meyatu/skills/. Returns the full skill prompt, tool permissions, and category. Use this to adopt a specialized skill for the current task.',
      parameters: {
        type: 'object',
        properties: {
          path: {
            type: 'string',
            description: 'Workspace directory path.',
          },
          name: {
            type: 'string',
            description: 'Skill name (e.g. "rust-expert").',
          },
        },
        required: ['path', 'name'],
      },
    },
    sideEffect: false,
    invoke: cmd('tool_skills_load', (a) => ({ path: a.path, name: a.name })),
  },

  // ── agents_list ──
  {
    definition: {
      name: 'agents_list',
      description:
        'List all configured parallel agents from .meyatu/agents.yml. Each agent has a name, description, system_prompt, and allowed tools. Use this to discover which specialized agents are available for parallel task execution.',
      parameters: {
        type: 'object',
        properties: {
          path: {
            type: 'string',
            description: 'Workspace directory path.',
          },
        },
        required: ['path'],
      },
    },
    sideEffect: false,
    invoke: cmd('tool_agents_list', (a) => ({ path: a.path })),
  },

  // ── ssh_exec ──
  {
    definition: {
      name: 'ssh_exec',
      description:
        'Execute a command on a remote server via SSH. Uses sshpass for password auth when available (falls back to key-based auth). Returns stdout, stderr, exit code, and a human-readable message. Common errors (connection refused, timeout, permission denied) are detected and explained in English.',
      parameters: {
        type: 'object',
        properties: {
          host: {
            type: 'string',
            description: 'Remote hostname or IP address.',
          },
          port: {
            type: 'number',
            description: 'SSH port (default: 22).',
          },
          username: {
            type: 'string',
            description: 'SSH username (default: root).',
          },
          password: {
            type: 'string',
            description: 'SSH password. Requires sshpass to be installed. Omit to use key-based auth.',
          },
          command: {
            type: 'string',
            description: 'Command to execute on the remote host (default: hostname).',
          },
        },
        required: ['host'],
      },
    },
    sideEffect: true,
    invoke: async (a) => {
      const params: Record<string, unknown> = {
        host: a.host,
        port: a.port,
        username: a.username,
        password: a.password,
        command: a.command,
      };
      const raw = await typedInvoke<string>('tool_ssh_exec', params);
      try {
        const parsed = JSON.parse(raw) as Record<string, unknown>;
        if (parsed && typeof parsed === 'object' && 'success' in parsed) {
          if (parsed.success) {
            const out = (parsed.stdout as string) || '';
            const err = (parsed.stderr as string) || '';
            return err ? `${out}\n${err}` : out;
          }
          return (parsed.message as string) || raw;
        }
      } catch { /* not JSON — return as-is */ }
      return raw;
    },
  },

  // ── index_codebase ──
  {
    definition: {
      name: 'index_codebase',
      description:
        'Scan and index the entire codebase. Returns file counts, language breakdown, package info, directory tree, entrypoints, and import graph. Use this to understand the project structure before making changes.',
      parameters: {
        type: 'object',
        properties: {
          path: {
            type: 'string',
            description: 'Workspace directory path to index.',
          },
        },
        required: ['path'],
      },
    },
    sideEffect: false,
    invoke: cmd('tool_index_codebase', (a) => ({ path: a.path })),
  },

  // ── impact_analysis ──
  {
    definition: {
      name: 'impact_analysis',
      description:
        'Analyze the impact of changing a symbol (function/class/type). Given a symbol name and file, finds all callers, callees, imports, and affected files. Use this BEFORE modifying important functions to understand what will break.',
      parameters: {
        type: 'object',
        properties: {
          path: { type: 'string', description: 'Workspace directory path.' },
          symbol: { type: 'string', description: 'Symbol name to analyze (function, class, type, etc.).' },
          line: { type: 'number', description: 'Line number of the definition (optional, helps disambiguate).' },
        },
        required: ['path', 'symbol'],
      },
    },
    sideEffect: false,
    invoke: cmd('tool_impact_analysis', (a) => ({
      path: a.path,
      symbol: a.symbol,
      line: a.line,
    })),
  },

  // ── code_graph ──
  {
    definition: {
      name: 'code_graph',
      description:
        'Build a code graph showing all definitions (functions, classes, structs, etc.), call relationships, and imports across the codebase. Returns nodes, edges, and module info. Use this for architecture visualization.',
      parameters: {
        type: 'object',
        properties: {
          path: { type: 'string', description: 'Workspace directory path.' },
        },
        required: ['path'],
      },
    },
    sideEffect: false,
    invoke: cmd('tool_code_graph', (a) => ({ path: a.path })),
  },

  // ── test_runner ──
  {
    definition: {
      name: 'test_runner',
      description:
        'Detect test framework and run tests. Supports jest, vitest, cargo test, pytest, go test, maven, gradle. Returns structured results with pass/fail counts, duration, and failure details. Use optional filter to run specific tests.',
      parameters: {
        type: 'object',
        properties: {
          path: { type: 'string', description: 'Workspace directory path.' },
          filter: { type: 'string', description: 'Optional test name/pattern filter.' },
          verbose: { type: 'boolean', description: 'Enable verbose output (default: false).' },
        },
        required: ['path'],
      },
    },
    sideEffect: true,
    invoke: (a) => {
      const params: Record<string, unknown> = { path: a.path };
      if (a.filter !== undefined) params.filter = a.filter;
      if (a.verbose !== undefined) params.verbose = a.verbose;
      return typedInvoke<string>('tool_test_runner', params);
    },
  },

  // ── doc_index ──
  {
    definition: {
      name: 'doc_index',
      description:
        'Scan and index all markdown/documentation files in the workspace. Returns file paths, titles, sections, and content structure. Use this to understand project documentation.',
      parameters: {
        type: 'object',
        properties: {
          path: { type: 'string', description: 'Workspace directory path.' },
        },
        required: ['path'],
      },
    },
    sideEffect: false,
    invoke: cmd('tool_doc_index', (a) => ({ path: a.path })),
  },

  // ── doc_search ──
  {
    definition: {
      name: 'doc_search',
      description:
        'Search through indexed documentation files. Returns matching sections with relevance scores and snippets. Use this to find relevant documentation.',
      parameters: {
        type: 'object',
        properties: {
          path: { type: 'string', description: 'Workspace directory path.' },
          query: { type: 'string', description: 'Search query string.' },
          limit: { type: 'number', description: 'Maximum results to return (default: 20).' },
        },
        required: ['path', 'query'],
      },
    },
    sideEffect: false,
    invoke: cmd('tool_doc_search', (a) => ({
      path: a.path,
      query: a.query,
      limit: a.limit,
    })),
  },

  // ── error_diagnosis ──
  {
    definition: {
      name: 'error_diagnosis',
      description:
        'Diagnose an error message and provide suggestions for fixing it. Matches against a library of known patterns (TypeScript, Rust, Python, Go) and returns contextual suggestions with code examples.',
      parameters: {
        type: 'object',
        properties: {
          error_message: { type: 'string', description: 'The error message to diagnose.' },
          file_path: { type: 'string', description: 'File where the error occurred (optional).' },
          context: { type: 'string', description: 'Additional context like stack trace (optional).' },
        },
        required: ['error_message'],
      },
    },
    sideEffect: false,
    invoke: (a) => {
      const params: Record<string, unknown> = { error_message: a.error_message };
      if (a.file_path !== undefined) params.file_path = a.file_path;
      if (a.context !== undefined) params.context = a.context;
      return typedInvoke<string>('tool_error_diagnosis', params);
    },
  },

  // ── perf_analyze ──
  {
    definition: {
      name: 'perf_analyze',
      description:
        'Analyze code for performance issues. Detects N+1 queries, blocking I/O in async, unnecessary cloning, regex in loops, and other anti-patterns. Returns issues with severity and optimization suggestions.',
      parameters: {
        type: 'object',
        properties: {
          path: { type: 'string', description: 'Workspace directory path.' },
          file_path: { type: 'string', description: 'Specific file to analyze (optional, analyzes all files if omitted).' },
        },
        required: ['path'],
      },
    },
    sideEffect: false,
    invoke: (a) => {
      const params: Record<string, unknown> = { path: a.path };
      if (a.file_path !== undefined) params.file_path = a.file_path;
      return typedInvoke<string>('tool_perf_analyze', params);
    },
  },

  // ── review_add ──
  {
    definition: {
      name: 'review_add',
      description:
        'Store a code review feedback item. Records file, line, comment, category (style/bug/performance/architecture/naming), and severity. Use this to track human review feedback.',
      parameters: {
        type: 'object',
        properties: {
          path: { type: 'string', description: 'Workspace directory path.' },
          file: { type: 'string', description: 'File path being reviewed.' },
          line: { type: 'number', description: 'Line number (optional).' },
          comment: { type: 'string', description: 'Review comment.' },
          category: { type: 'string', description: 'Category: style, bug, performance, architecture, naming.' },
          severity: { type: 'string', description: 'Severity: critical, major, minor, suggestion.' },
        },
        required: ['path', 'file', 'comment', 'category', 'severity'],
      },
    },
    sideEffect: false,
    invoke: cmd('tool_review_add', (a) => ({
      path: a.path,
      file: a.file,
      line: a.line,
      comment: a.comment,
      category: a.category,
      severity: a.severity,
    })),
  },

  // ── review_list ──
  {
    definition: {
      name: 'review_list',
      description:
        'List all code review feedback items, optionally filtered by file or category. Returns feedbacks with stats.',
      parameters: {
        type: 'object',
        properties: {
          path: { type: 'string', description: 'Workspace directory path.' },
          file: { type: 'string', description: 'Filter by file path (optional).' },
          category: { type: 'string', description: 'Filter by category (optional).' },
          unresolved_only: { type: 'boolean', description: 'Show only unresolved items (default: false).' },
        },
        required: ['path'],
      },
    },
    sideEffect: false,
    invoke: (a) => {
      const params: Record<string, unknown> = { path: a.path };
      if (a.file !== undefined) params.file = a.file;
      if (a.category !== undefined) params.category = a.category;
      if (a.unresolved_only !== undefined) params.unresolved_only = a.unresolved_only;
      return typedInvoke<string>('tool_review_list', params);
    },
  },

  // ── review_resolve ──
  {
    definition: {
      name: 'review_resolve',
      description:
        'Mark a review feedback item as resolved.',
      parameters: {
        type: 'object',
        properties: {
          path: { type: 'string', description: 'Workspace directory path.' },
          review_id: { type: 'string', description: 'ID of the review to resolve.' },
        },
        required: ['path', 'review_id'],
      },
    },
    sideEffect: false,
    invoke: cmd('tool_review_resolve', (a) => ({
      path: a.path,
      review_id: a.review_id,
    })),
  },

  // ── review_stats ──
  {
    definition: {
      name: 'review_stats',
      description:
        'Get code review statistics: total/unresolved counts, breakdown by category and severity.',
      parameters: {
        type: 'object',
        properties: {
          path: { type: 'string', description: 'Workspace directory path.' },
        },
        required: ['path'],
      },
    },
    sideEffect: false,
    invoke: cmd('tool_review_stats', (a) => ({ path: a.path })),
  },

  // ── global_register_project ──
  {
    definition: {
      name: 'global_register_project',
      description:
        'Register a project in the global memory system. Tracks language, framework, and enables cross-project learning.',
      parameters: {
        type: 'object',
        properties: {
          path: { type: 'string', description: 'Workspace directory path.' },
          name: { type: 'string', description: 'Project name.' },
          language: { type: 'string', description: 'Primary language (rust, typescript, python, etc.).' },
          framework: { type: 'string', description: 'Framework if any (react, nextjs, actix, etc.).' },
        },
        required: ['path', 'name', 'language'],
      },
    },
    sideEffect: false,
    invoke: (a) =>
      typedInvoke<string>('tool_global_register_project', {
        path: a.path,
        name: a.name,
        language: a.language,
        framework: a.framework,
      }),
  },

  // ── global_add_note ──
  {
    definition: {
      name: 'global_add_note',
      description:
        'Add a note to the current project memory. Categories: best_practice, gotcha, tip, architecture.',
      parameters: {
        type: 'object',
        properties: {
          path: { type: 'string', description: 'Workspace directory path.' },
          content: { type: 'string', description: 'Note content.' },
          category: { type: 'string', description: 'Category: best_practice, gotcha, tip, architecture.' },
          tags: { type: 'array', items: { type: 'string' }, description: 'Tags for categorization.' },
        },
        required: ['path', 'content', 'category'],
      },
    },
    sideEffect: false,
    invoke: cmd('tool_global_add_note', (a) => ({
      path: a.path,
      content: a.content,
      category: a.category,
      tags: a.tags,
    })),
  },

  // ── global_search ──
  {
    definition: {
      name: 'global_search',
      description:
        'Search across all project memories. Find notes by content or tags.',
      parameters: {
        type: 'object',
        properties: {
          query: { type: 'string', description: 'Search query.' },
          language: { type: 'string', description: 'Filter by language (optional).' },
          category: { type: 'string', description: 'Filter by category (optional).' },
          limit: { type: 'number', description: 'Max results (default: 20).' },
        },
        required: ['query'],
      },
    },
    sideEffect: false,
    invoke: (a) => {
      const params: Record<string, unknown> = { query: a.query };
      if (a.language !== undefined) params.language = a.language;
      if (a.category !== undefined) params.category = a.category;
      if (a.limit !== undefined) params.limit = a.limit;
      return typedInvoke<string>('tool_global_search', params);
    },
  },

  // ── global_add_pattern ──
  {
    definition: {
      name: 'global_add_pattern',
      description:
        'Add a shared pattern learned from this project. Patterns are reusable best practices across projects.',
      parameters: {
        type: 'object',
        properties: {
          path: { type: 'string', description: 'Workspace directory path.' },
          pattern: { type: 'string', description: 'Pattern name/title.' },
          description: { type: 'string', description: 'Pattern description.' },
          language: { type: 'string', description: 'Applicable language (optional).' },
          framework: { type: 'string', description: 'Applicable framework (optional).' },
          examples: { type: 'array', items: { type: 'string' }, description: 'Code examples.' },
        },
        required: ['path', 'pattern', 'description'],
      },
    },
    sideEffect: false,
    invoke: cmd('tool_global_add_pattern', (a) => ({
      path: a.path,
      pattern: a.pattern,
      description: a.description,
      language: a.language,
      framework: a.framework,
      examples: a.examples,
    })),
  },

  // ── dispatch_parallel_agents ──
  // Intercepted by loop.ts; the old switch default threw. sideEffect=false (not in legacy set).
  {
    definition: {
      name: 'dispatch_parallel_agents',
      description:
        'Run several independent sub-tasks concurrently as parallel agents and get '
        + 'their combined results. Use for independent work that benefits from fan-out '
        + '(e.g. explore N modules, review N files). Each task runs its own agent loop. '
        + 'Do not use for dependent/sequential work.',
      parameters: {
        type: 'object',
        properties: {
          tasks: {
            type: 'array',
            description: 'Independent sub-tasks to run in parallel.',
            items: {
              type: 'object',
              properties: {
                task: { type: 'string', description: 'The instruction for this sub-agent.' },
                agent: {
                  type: 'string',
                  description:
                    'Optional named agent from .meyatu/agents.yml; uses its system_prompt. '
                    + 'Omit to use the default assistant prompt.',
                },
              },
              required: ['task'],
              additionalProperties: false,
            },
          },
        },
        required: ['tasks'],
        additionalProperties: false,
      },
    },
    sideEffect: false,
    invoke: async () => { throw new Error('Unknown tool: dispatch_parallel_agents'); },
  },

  // ── web_scrape ──
  {
    definition: {
      name: 'web_scrape',
      description:
        'Fetch a single web page and return its content as clean Markdown. ' +
        'Handles JavaScript-rendered pages when Chrome is available locally. ' +
        'Use for reading documentation, blog posts, news, or any single URL.',
      parameters: {
        type: 'object',
        properties: {
          url: { type: 'string', description: 'The full URL to scrape, including https://.' },
        },
        required: ['url'],
      },
    },
    sideEffect: false,
    invoke: cmd('tool_web_scrape', (a) => ({ url: a.url })),
  },

  // ── web_crawl ──
  {
    definition: {
      name: 'web_crawl',
      description:
        'Crawl multiple pages from a starting URL using BFS. Each page is ' +
        'fetched and extracted as Markdown, separated by page URL headings. ' +
        'Only follows same-domain links. Use for documentation sites with ' +
        'multiple pages or exploring a website structure.',
      parameters: {
        type: 'object',
        properties: {
          url: { type: 'string', description: 'Starting URL to crawl from.' },
          max_pages: { type: 'integer', description: 'Maximum pages to crawl (default 5).' },
        },
        required: ['url'],
      },
    },
    sideEffect: false,
    invoke: cmd('tool_web_crawl', (a) => ({ url: a.url, max_pages: a.max_pages })),
  },

  // ── web_map ──
  {
    definition: {
      name: 'web_map',
      description:
        'Discover all same-domain URLs reachable from a starting page. ' +
        'Returns a JSON array of URLs. Useful for understanding site structure, ' +
        'finding endpoints, or building a sitemap from a landing page.',
      parameters: {
        type: 'object',
        properties: {
          url: { type: 'string', description: 'Starting URL to discover links from.' },
          limit: { type: 'integer', description: 'Maximum URLs to return (default 20).' },
        },
        required: ['url'],
      },
    },
    sideEffect: false,
    invoke: cmd('tool_web_map', (a) => ({ url: a.url, limit: a.limit })),
  },
];

export const REGISTRY_DEFS: ToolDefinition[] = TOOL_REGISTRY.map((s) => s.definition);

const REGISTRY_MAP = new Map(TOOL_REGISTRY.map((s) => [s.definition.name, s]));

/** Whether a built-in tool mutates state (registry-derived). */
export function hasSideEffect(toolName: string): boolean {
  return REGISTRY_MAP.get(toolName)?.sideEffect ?? false;
}
