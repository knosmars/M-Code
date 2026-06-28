import { useEffect, useState } from 'react';
import { useMcpStore } from '../../stores/mcpStore';
import { useT } from '../../i18n/useT';
import shared from './settings.module.css';

function parseArgs(raw: string): string[] {
  return raw.trim() ? raw.trim().split(/\s+/) : [];
}

function parseEnv(raw: string): Record<string, string> {
  const env: Record<string, string> = {};
  for (const line of raw.split('\n')) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    const eq = trimmed.indexOf('=');
    if (eq > 0) env[trimmed.slice(0, eq).trim()] = trimmed.slice(eq + 1).trim();
  }
  return env;
}

export function McpServersSection() {
  const t = useT();
  const { servers, statuses, error, load, addServer, removeServer, setDisabled, toolsForServer } =
    useMcpStore();
  const [name, setName] = useState('');
  const [command, setCommand] = useState('');
  const [args, setArgs] = useState('');
  const [env, setEnv] = useState('');
  const [expanded, setExpanded] = useState<string | null>(null);

  useEffect(() => {
    void load();
  }, [load]);

  const statusOf = (n: string) => statuses.find((s) => s.name === n);

  const onAdd = async () => {
    try {
      await addServer(name.trim(), command.trim(), parseArgs(args), parseEnv(env));
      setName('');
      setCommand('');
      setArgs('');
      setEnv('');
    } catch {
      // error surfaced via store.error; keep form input
    }
  };

  return (
    <div className={shared.section}>
      <h2>{t('mcp.title')}</h2>
      <p className={shared.sectionDesc}>
        {t('mcp.descBefore')}<code>mcp__&lt;server&gt;__&lt;tool&gt;</code>{t('mcp.descAfter')}
      </p>

      {error && <div className={shared.error} role="alert">{error}</div>}

      {servers.length === 0 && <p className={shared.sectionDesc}>{t('mcp.empty')}</p>}

      {servers.map((srv) => {
        const st = statusOf(srv.name);
        const dotColor = srv.disabled ? '#888' : st?.connected ? '#3fb950' : '#d29922';
        const tools = toolsForServer(srv.name);
        return (
          <div
            key={srv.name}
            className={shared.row}
            style={{ flexDirection: 'column', alignItems: 'stretch' }}
          >
            <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <span
                style={{
                  width: 8,
                  height: 8,
                  borderRadius: '50%',
                  background: dotColor,
                  display: 'inline-block',
                }}
              />
              <strong>{srv.name}</strong>
              <span className={shared.sectionDesc} style={{ margin: 0 }}>{srv.command}</span>
              <span style={{ marginLeft: 'auto' }}>{t('mcp.toolCount', { n: st?.toolCount ?? 0 })}</span>
              <button onClick={() => setExpanded(expanded === srv.name ? null : srv.name)}>
                {expanded === srv.name ? t('mcp.collapse') : t('mcp.tools')}
              </button>
              <label className={shared.toggle} style={{ margin: 0 }}>
                <input
                  type="checkbox"
                  checked={!srv.disabled}
                  onChange={(e) => void setDisabled(srv.name, !e.target.checked)}
                />
                <span className={shared.toggleLabel}>{t('mcp.enable')}</span>
              </label>
              <button onClick={() => void removeServer(srv.name)}>{t('mcp.remove')}</button>
            </div>
            {expanded === srv.name && (
              <ul style={{ margin: '8px 0 0 16px' }}>
                {tools.length === 0 && <li className={shared.sectionDesc}>{t('mcp.noTools')}</li>}
                {tools.map((tool) => (
                  <li key={tool.name}>
                    <code>{tool.name}</code> — {tool.description}
                  </li>
                ))}
              </ul>
            )}
          </div>
        );
      })}

      <div
        className={shared.row}
        style={{ flexDirection: 'column', alignItems: 'stretch', gap: 6 }}
      >
        <input placeholder={t('mcp.phName')} value={name} onChange={(e) => setName(e.target.value)} />
        <input placeholder={t('mcp.phCommand')} value={command} onChange={(e) => setCommand(e.target.value)} />
        <input placeholder={t('mcp.phArgs')} value={args} onChange={(e) => setArgs(e.target.value)} />
        <textarea
          placeholder={t('mcp.phEnv')}
          value={env}
          onChange={(e) => setEnv(e.target.value)}
          rows={2}
        />
        <button onClick={() => void onAdd()} disabled={!name.trim() || !command.trim()}>
          {t('mcp.add')}
        </button>
      </div>
    </div>
  );
}
