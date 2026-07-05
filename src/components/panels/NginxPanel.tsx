import { useState, useEffect, useCallback } from 'react'
import { invoke } from '@tauri-apps/api/core'

interface ServiceInfo {
  name: string
  display_name: string
  active: boolean
  status_text: string
  version: string
  pid: string
  memory: string
  uptime: string
  config_path: string
}

interface NginxPanelProps {
  sessionId: string | null
}

type Tab = 'status' | 'config' | 'vhosts' | 'logs'

export default function NginxPanel({ sessionId }: NginxPanelProps) {
  const [tab, setTab] = useState<Tab>('status')
  const [info, setInfo] = useState<ServiceInfo | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [actionLoading, setActionLoading] = useState('')

  const fetchInfo = useCallback(async () => {
    if (!sessionId) return
    setLoading(true)
    setError('')
    try {
      const svcInfo = await invoke<ServiceInfo>('server_get_service_info', {
        sessionId,
        service: 'nginx',
      })
      setInfo(svcInfo)
    } catch (e) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }, [sessionId])

  useEffect(() => {
    fetchInfo()
  }, [fetchInfo])

  const handleAction = async (action: string) => {
    if (!sessionId) return
    setActionLoading(action)
    try {
      await invoke('server_service_action', { sessionId, service: 'nginx', action })
      setTimeout(fetchInfo, 1000)
    } catch (e) {
      setError(`Action failed: ${e}`)
    } finally {
      setActionLoading('')
    }
  }

  if (!sessionId) return <div className="sp-empty">Connect to a server first</div>
  if (loading && !info) return <div className="sp-loading">Loading Nginx info...</div>

  return (
    <div className="svc-panel">
      {/* Header */}
      <div className="svc-header">
        <div className="svc-title">
          <h2>Nginx</h2>
          {info && (
            <>
              <span className={`svc-status-badge ${info.active ? 'active' : 'inactive'}`}>
                {info.active ? 'Running' : 'Stopped'}
              </span>
              {info.version && <span className="svc-version">v{info.version}</span>}
            </>
          )}
        </div>
        <div className="svc-actions">
          {info && !info.active && (
            <ActionBtn label="Start" action="start" onClick={handleAction} loading={actionLoading} />
          )}
          {info && info.active && (
            <>
              <ActionBtn label="Reload" action="reload" onClick={handleAction} loading={actionLoading} primary />
              <ActionBtn label="Restart" action="restart" onClick={handleAction} loading={actionLoading} />
              <ActionBtn label="Stop" action="stop" onClick={handleAction} loading={actionLoading} danger />
            </>
          )}
        </div>
      </div>

      {error && <div className="svc-error">{error}</div>}

      {/* Tabs */}
      <div className="svc-tabs">
        {(['status', 'config', 'vhosts', 'logs'] as Tab[]).map((t) => (
          <button
            key={t}
            className={`svc-tab ${tab === t ? 'active' : ''}`}
            onClick={() => setTab(t)}
          >
            {t === 'status' ? 'Status' : t === 'config' ? 'Config' : t === 'vhosts' ? 'VHosts' : 'Logs'}
          </button>
        ))}
      </div>

      {/* Tab Content */}
      <div className="svc-tab-content">
        {tab === 'status' && info && <StatusTab info={info} />}
        {tab === 'config' && <ConfigTab sessionId={sessionId} configPath={info?.config_path || '/etc/nginx/nginx.conf'} />}
        {tab === 'vhosts' && <VhostsTab sessionId={sessionId} />}
        {tab === 'logs' && <LogsTab sessionId={sessionId} />}
      </div>
    </div>
  )
}

// ===== Sub-components =====

function ActionBtn({
  label, action, onClick, loading, primary, danger,
}: {
  label: string; action: string; onClick: (a: string) => void
  loading: string; primary?: boolean; danger?: boolean
}) {
  return (
    <button
      className={`svc-action-btn ${primary ? 'primary' : ''} ${danger ? 'danger' : ''}`}
      onClick={() => onClick(action)}
      disabled={loading === action}
    >
      {loading === action ? '...' : label}
    </button>
  )
}

function StatusTab({ info }: { info: ServiceInfo }) {
  return (
    <div className="svc-status-grid">
      <InfoRow label="Status" value={info.status_text} />
      <InfoRow label="PID" value={info.pid || '-'} />
      <InfoRow label="Memory" value={info.memory || '-'} />
      <InfoRow label="Uptime" value={info.uptime || '-'} />
      <InfoRow label="Version" value={info.version || '-'} />
      <InfoRow label="Config" value={info.config_path} mono />
    </div>
  )
}

function InfoRow({ label, value, mono }: { label: string; value: string; mono?: boolean }) {
  return (
    <div className="svc-info-row">
      <span className="svc-info-label">{label}</span>
      <span className={`svc-info-value ${mono ? 'mono' : ''}`}>{value}</span>
    </div>
  )
}

function ConfigTab({ sessionId, configPath }: { sessionId: string; configPath: string }) {
  const [content, setContent] = useState('')
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [dirty, setDirty] = useState(false)
  const [testResult, setTestResult] = useState<{ ok: boolean; msg: string } | null>(null)
  const [msg, setMsg] = useState('')

  const loadConfig = async () => {
    setLoading(true)
    try {
      const text = await invoke<string>('server_read_remote_file', { sessionId, path: configPath })
      setContent(text)
      setDirty(false)
    } catch (e) {
      setMsg(String(e))
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => { loadConfig() }, [sessionId, configPath])

  const handleSave = async () => {
    setSaving(true)
    setMsg('')
    try {
      await invoke('server_write_remote_file', { sessionId, path: configPath, content })
      setDirty(false)
      setMsg('Saved successfully')
    } catch (e) {
      setMsg(`Save failed: ${e}`)
    } finally {
      setSaving(false)
    }
  }

  const handleTest = async () => {
    try {
      const [ok, resultMsg] = await invoke<[boolean, string]>('server_test_nginx_config', { sessionId })
      setTestResult({ ok, msg: resultMsg })
    } catch (e) {
      setTestResult({ ok: false, msg: String(e) })
    }
  }

  if (loading) return <div className="svc-loading">Loading config...</div>

  return (
    <div className="svc-config">
      <div className="svc-config-toolbar">
        <span className="svc-config-path">{configPath}</span>
        <div className="svc-config-btns">
          <button className="svc-cfg-btn" onClick={handleTest}>Test Config</button>
          <button className="svc-cfg-btn primary" onClick={handleSave} disabled={!dirty || saving}>
            {saving ? 'Saving...' : 'Save'}
          </button>
        </div>
      </div>
      {testResult && (
        <div className={`svc-test-result ${testResult.ok ? 'ok' : 'fail'}`}>
          {testResult.msg}
        </div>
      )}
      {msg && <div className="svc-msg">{msg}</div>}
      <textarea
        className="svc-config-editor"
        value={content}
        onChange={(e) => { setContent(e.target.value); setDirty(true) }}
        spellCheck={false}
      />
    </div>
  )
}

function VhostsTab({ sessionId }: { sessionId: string }) {
  const [vhosts, setVhosts] = useState<string[]>([])
  const [selected, setSelected] = useState<string | null>(null)
  const [content, setContent] = useState('')
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [dirty, setDirty] = useState(false)
  const [msg, setMsg] = useState('')

  const loadVhosts = async () => {
    setLoading(true)
    try {
      const list = await invoke<string[]>('server_list_nginx_vhosts', { sessionId })
      setVhosts(list)
    } catch (e) {
      setMsg(String(e))
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => { loadVhosts() }, [sessionId])

  const loadVhost = async (path: string) => {
    setSelected(path)
    setMsg('')
    try {
      const text = await invoke<string>('server_read_remote_file', { sessionId, path })
      setContent(text)
      setDirty(false)
    } catch (e) {
      setMsg(String(e))
    }
  }

  const handleSave = async () => {
    if (!selected) return
    setSaving(true)
    try {
      await invoke('server_write_remote_file', { sessionId, path: selected, content })
      setDirty(false)
      setMsg('Saved')
    } catch (e) {
      setMsg(`Save failed: ${e}`)
    } finally {
      setSaving(false)
    }
  }

  if (loading) return <div className="svc-loading">Loading vhosts...</div>

  return (
    <div className="svc-vhosts">
      <div className="svc-vhosts-list">
        <div className="svc-vhosts-title">Virtual Hosts ({vhosts.length})</div>
        {vhosts.length === 0 && <div className="svc-empty">No vhosts found</div>}
        {vhosts.map((v) => (
          <button
            key={v}
            className={`svc-vhost-item ${selected === v ? 'active' : ''}`}
            onClick={() => loadVhost(v)}
          >
            {v.split('/').pop()}
          </button>
        ))}
      </div>
      {selected && (
        <div className="svc-vhost-editor">
          <div className="svc-config-toolbar">
            <span className="svc-config-path">{selected}</span>
            <button className="svc-cfg-btn primary" onClick={handleSave} disabled={!dirty || saving}>
              {saving ? 'Saving...' : 'Save'}
            </button>
          </div>
          {msg && <div className="svc-msg">{msg}</div>}
          <textarea
            className="svc-config-editor"
            value={content}
            onChange={(e) => { setContent(e.target.value); setDirty(true) }}
            spellCheck={false}
          />
        </div>
      )}
    </div>
  )
}

function LogsTab({ sessionId }: { sessionId: string }) {
  const [logPath, setLogPath] = useState('/var/log/nginx/error.log')
  const [logContent, setLogContent] = useState('')
  const [loading, setLoading] = useState(false)

  const loadLog = async () => {
    setLoading(true)
    try {
      const text = await invoke<string>('server_get_log_lines', {
        sessionId, path: logPath, lines: 100,
      })
      setLogContent(text || '(empty)')
    } catch (e) {
      setLogContent(`Error: ${e}`)
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => { loadLog() }, [sessionId, logPath])

  return (
    <div className="svc-logs">
      <div className="svc-logs-toolbar">
        <select
          className="svc-log-select"
          value={logPath}
          onChange={(e) => setLogPath(e.target.value)}
        >
          <option value="/var/log/nginx/error.log">error.log</option>
          <option value="/var/log/nginx/access.log">access.log</option>
        </select>
        <button className="svc-cfg-btn" onClick={loadLog} disabled={loading}>
          {loading ? 'Loading...' : 'Refresh'}
        </button>
      </div>
      <pre className="svc-log-content">{logContent}</pre>
    </div>
  )
}
