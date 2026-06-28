import { describe, it, expect, vi, beforeEach } from 'vitest';

const typedInvoke = vi.fn();
vi.mock('../utils/ipc', () => ({
  typedInvoke: (cmd: string, args?: Record<string, unknown>) => typedInvoke(cmd, args),
  normalizeError: (e: unknown) => ({
    code: 'internal',
    message: e instanceof Error ? e.message : String(e),
    retryable: false,
    retryAfter: null,
  }),
}));

const registerInterest = vi.fn().mockResolvedValue(undefined);
const publishEvent = vi.fn().mockResolvedValue(undefined);
vi.mock('../stores/fileSyncStore', () => ({
  useFileSyncStore: { getState: () => ({ registerInterest, publishEvent }) },
}));

import { TauriToolExecutor } from './toolExecutor';

function call(name: string, args: Record<string, unknown>) {
  const exec = new TauriToolExecutor();
  return exec.execute({ id: 't', name, arguments: JSON.stringify(args) });
}

beforeEach(() => {
  typedInvoke.mockReset();
  typedInvoke.mockResolvedValue('ok');
  registerInterest.mockClear();
  publishEvent.mockClear();
});

describe('dispatch trivial passthroughs', () => {
  it('list_dir → tool_list_dir', async () => {
    await call('list_dir', { path: '/x' });
    expect(typedInvoke).toHaveBeenCalledWith('tool_list_dir', { path: '/x' });
  });
  it('git_status → tool_git_status', async () => {
    await call('git_status', { path: '/r' });
    expect(typedInvoke).toHaveBeenCalledWith('tool_git_status', { path: '/r' });
  });
  it('memory_write → tool_memory_write with mode', async () => {
    await call('memory_write', { path: 'p', content: 'c', mode: 'append' });
    expect(typedInvoke).toHaveBeenCalledWith('tool_memory_write', { path: 'p', content: 'c', mode: 'append' });
  });
});

describe('dispatch special behaviors', () => {
  it('read_file registers fileSync interest', async () => {
    typedInvoke.mockResolvedValue('contents');
    const out = await call('read_file', { path: '/f' });
    expect(registerInterest).toHaveBeenCalledWith('chat', '/f');
    expect(typedInvoke).toHaveBeenCalledWith('tool_read_file', { path: '/f' });
    expect(out.content).toBe('contents');
  });
  it('write_file registers interest + publishes modified', async () => {
    await call('write_file', { path: '/f', content: 'x' });
    expect(registerInterest).toHaveBeenCalledWith('chat', '/f');
    expect(publishEvent).toHaveBeenCalledWith('/f', 'modified', 'chat');
    expect(typedInvoke).toHaveBeenCalledWith('tool_write_file', { path: '/f', content: 'x' });
  });
  it('edit_file registers interest + publishes modified + maps oldString/newString', async () => {
    await call('edit_file', { path: '/f', oldString: 'a', newString: 'b' });
    expect(registerInterest).toHaveBeenCalledWith('chat', '/f');
    expect(publishEvent).toHaveBeenCalledWith('/f', 'modified', 'chat');
    expect(typedInvoke).toHaveBeenCalledWith('tool_edit_file', { path: '/f', oldString: 'a', newString: 'b' });
  });
  it('grep defaults path to "." when absent', async () => {
    await call('grep', { pattern: 'foo' });
    expect(typedInvoke).toHaveBeenCalledWith('tool_grep', { pattern: 'foo', path: '.', include: undefined });
  });
  it('search_codebase defaults path to "."', async () => {
    await call('search_codebase', { query: 'q' });
    expect(typedInvoke).toHaveBeenCalledWith('tool_search_codebase', { query: 'q', path: '.' });
  });
  it('semantic_search forces path "." and maps top_k→topK', async () => {
    await call('semantic_search', { query: 'q', top_k: 3 });
    expect(typedInvoke).toHaveBeenCalledWith('tool_semantic_search', { query: 'q', path: '.', topK: 3 });
  });
  it('git_branch omits create when absent / includes when present', async () => {
    await call('git_branch', { path: '/r', name: 'b' });
    expect(typedInvoke).toHaveBeenCalledWith('tool_git_branch', { path: '/r', name: 'b' });
    typedInvoke.mockClear();
    await call('git_branch', { path: '/r', name: 'b', create: true });
    expect(typedInvoke).toHaveBeenCalledWith('tool_git_branch', { path: '/r', name: 'b', create: true });
  });
  it('gh_pr_create includes optional body/base only when present', async () => {
    await call('gh_pr_create', { path: '/r', title: 't' });
    expect(typedInvoke).toHaveBeenCalledWith('tool_gh_pr_create', { path: '/r', title: 't' });
  });
  it('run_command returns stderr text on failure marker', async () => {
    typedInvoke.mockResolvedValue('boom\n[exit code: 1]\n[stderr]\nreal error');
    const out = await call('run_command', { command: 'x' });
    expect(out.content).toContain('real error');
  });
});

describe('hasSideEffect', () => {
  it('write/run/git_commit are side effects; read/list are not', () => {
    const exec = new TauriToolExecutor();
    for (const t of ['write_file', 'edit_file', 'run_command', 'git_commit', 'git_branch', 'git_push', 'gh_pr_create', 'hooks_run', 'triggers_watch', 'triggers_start_auto', 'ssh_exec', 'terminal_start', 'terminal_send', 'terminal_stop', 'generate_image', 'test_runner']) {
      expect(exec.hasSideEffect(t)).toBe(true);
    }
    for (const t of ['read_file', 'list_dir', 'grep', 'git_status', 'memory_write', 'semantic_search']) {
      expect(exec.hasSideEffect(t)).toBe(false);
    }
  });
  it('mcp__ tools are always side effects', () => {
    expect(new TauriToolExecutor().hasSideEffect('mcp__srv__do')).toBe(true);
  });
});

describe('getDefinitions matches snapshot', () => {
  it('built-in definitions equal the committed BASE_TOOLS snapshot', async () => {
    const snap = (await import('./__fixtures__/base-tools.snapshot.json')).default;
    const exec = new TauriToolExecutor();
    expect(JSON.stringify(exec.getDefinitions())).toBe(JSON.stringify(snap));
  });
});
