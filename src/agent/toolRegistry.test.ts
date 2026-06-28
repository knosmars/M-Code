import { describe, it, expect } from 'vitest';
import { TOOL_REGISTRY, REGISTRY_DEFS } from './toolRegistry';
import snapshot from './__fixtures__/base-tools.snapshot.json';

const SIDE_EFFECT = new Set([
  'write_file', 'edit_file', 'run_command', 'git_commit', 'git_branch', 'git_push',
  'gh_pr_create', 'hooks_run', 'triggers_watch', 'triggers_start_auto', 'ssh_exec',
  'terminal_start', 'terminal_send', 'terminal_stop', 'generate_image', 'test_runner',
]);

describe('registry parity with BASE_TOOLS snapshot', () => {
  it('definitions deep-equal the committed snapshot (same order)', () => {
    expect(JSON.stringify(REGISTRY_DEFS)).toBe(JSON.stringify(snapshot));
  });
  it('every registry tool name is unique', () => {
    const names = TOOL_REGISTRY.map((s) => s.definition.name);
    expect(new Set(names).size).toBe(names.length);
  });
  it('sideEffect flags match the legacy SIDE_EFFECT_TOOLS set', () => {
    for (const s of TOOL_REGISTRY) {
      expect(s.sideEffect).toBe(SIDE_EFFECT.has(s.definition.name));
    }
  });
});
