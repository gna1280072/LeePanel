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

interface PhpPanelProps {
  sessionId: string | null
}

type Tab = 'status' | 'config' | 'fpm' | 'logs'

export default function PhpPanel({ sessionId }: PhpPanelProps) {
  const [tab, setTab] = useState<Tab>('status')
  const [info, setInfo] = useState<ServiceInfo | null>(null)
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [actionLoading, setActionLoading] = useState('')
  const [serviceName, setServiceName] = useState('php-fpm')

  const fetchInfo = useCallback(async () => {
    if (!sessionId) return
    setLoading(true)
    setError('')
    try {
      const [svcName, svcInfo] = await invoke<[string, ServiceInfo]>('server_find_php_service', { sessionId })
      setServiceName(svcName)
      setInfo(svcInfo)
    } catch (e) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }, [sessionId])

  useEffect(() => { fetchInfo() }, [fetchInfo])

  const handleAction = async (action: string) => {
    if (!sessionId) return
    setActionLoading(action)
    try {
      await invoke('server_service_action', { sessionId, service: serviceName, action })
      setTimeout(fetchInfo, 1000)
    } catch (e) {
      setError(`Action failed: ${e}`)
    } finally {
      setActionLoading('')
    }
  }

  if (!sessionId) return <div className="sp-empty">Connect to a server first</div>
  if (loading && !info) return <div className="sp-loading">Loading PHP info...</div>

  return (
    <div className="svc-panel">
      <div className="svc-header">
        <div className="svc-title">
          <h2>PHP-FPM</h2>
          {info && (
            <>
              <span className={`svc-status-badge ${info.active ? 'active' : 'inactive'}`}>
                {info.active ? 'Running' : 'Stopped'}
              </span>
              {info.version && <span className="svc-version">PHP {info.version}</span>}
            </>
          )}
        </div>
        <div className="svc-actions">
          {info && !info.active && (
            <ActionBtn label="Start" action="start" onClick={handleAction} loading={actionLoading} />
          )}
          {info && info.active && (
            <>
              <ActionBtn label="Restart" action="restart" onClick={handleAction} loading={actionLoading} primary />
              <ActionBtn label="Reload" action="reload" onClick={handleAction} loading={actionLoading} />
              <ActionBtn label="Stop" action="stop" onClick={handleAction} loading={actionLoading} danger />
            </>
          )}
        </div>
      </div>

      {error && <div className="svc-error">{error}</div>}

      <div className="svc-tabs">
        {(['status', 'config', 'fpm', 'logs'] as Tab[]).map((t) => (
          <button
            key={t}
            className={`svc-tab ${tab === t ? 'active' : ''}`}
            onClick={() => setTab(t)}
          >
            {t === 'status' ? 'Status' : t === 'config' ? 'php.ini' : t === 'fpm' ? 'FPM Config' : 'Logs'}
          </button>
        ))}
      </div>

      <div className="svc-tab-content">
        {tab === 'status' && info && <StatusTab info={info} sessionId={sessionId} />}
        {tab === 'config' && <ConfigTab sessionId={sessionId} configPath={info?.config_path || '/etc/php.ini'} />}
        {tab === 'fpm' && <FpmConfigTab sessionId={sessionId} />}
        {tab === 'logs' && <LogsTab sessionId={sessionId} />}
      </div>
    </div>
  )
}

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

function StatusTab({ info }: { info: ServiceInfo; sessionId: string }) {

  return (
    <div className="svc-status-grid">
      <InfoRow label="Status" value={info.status_text} />
      <InfoRow label="PID" value={info.pid || '-'} />
      <InfoRow label="Memory" value={info.memory || '-'} />
      <InfoRow label="Uptime" value={info.uptime || '-'} />
      <InfoRow label="PHP Version" value={info.version || '-'} />
      <InfoRow label="php.ini" value={info.config_path} mono />
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
  const [msg, setMsg] = useState('')

  useEffect(() => {
    (async () => {
      setLoading(true)
      try {
        const text = await invoke<string>('server_read_remote_file', { sessionId, path: configPath })
        setContent(text)
      } catch (e) {
        setMsg(String(e))
      } finally {
        setLoading(false)
      }
    })()
  }, [sessionId, configPath])

  const handleSave = async () => {
    setSaving(true)
    try {
      await invoke('server_write_remote_file', { sessionId, path: configPath, content })
      setDirty(false)
      setMsg('Saved. Restart PHP-FPM to apply changes.')
    } catch (e) {
      setMsg(`Save failed: ${e}`)
    } finally {
      setSaving(false)
    }
  }

  if (loading) return <div className="svc-loading">Loading php.ini...</div>

  return (
    <div className="svc-config">
      <div className="svc-config-toolbar">
        <span className="svc-config-path">{configPath}</span>
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
  )
}

function FpmConfigTab({ sessionId }: { sessionId: string }) {
  const [content, setContent] = useState('')
  const [fpmPath, setFpmPath] = useState('')
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [dirty, setDirty] = useState(false)
  const [msg, setMsg] = useState('')

  useEffect(() => {
    (async () => {
      setLoading(true)
      try {
        const [path, text] = await invoke<[string, string]>('server_find_php_fpm_config', { sessionId })
        setContent(text)
        setFpmPath(path)
      } catch (e) {
        setMsg(String(e))
      } finally {
        setLoading(false)
      }
    })()
  }, [sessionId])

  const handleSave = async () => {
    if (!fpmPath) return
    setSaving(true)
    try {
      await invoke('server_write_remote_file', { sessionId, path: fpmPath, content })
      setDirty(false)
      setMsg('Saved. Restart PHP-FPM to apply.')
    } catch (e) {
      setMsg(`Save failed: ${e}`)
    } finally {
      setSaving(false)
    }
  }

  if (loading) return <div className="svc-loading">Loading FPM config...</div>

  return (
    <div className="svc-config">
      <div className="svc-config-toolbar">
        <span className="svc-config-path">{fpmPath || 'Not found'}</span>
        {fpmPath && (
          <button className="svc-cfg-btn primary" onClick={handleSave} disabled={!dirty || saving}>
            {saving ? 'Saving...' : 'Save'}
          </button>
        )}
      </div>
      {msg && <div className="svc-msg">{msg}</div>}
      {fpmPath && (
        <textarea
          className="svc-config-editor"
          value={content}
          onChange={(e) => { setContent(e.target.value); setDirty(true) }}
          spellCheck={false}
        />
      )}
    </div>
  )
}

function LogsTab({ sessionId }: { sessionId: string }) {
  const [logPath, setLogPath] = useState('/var/log/php-fpm/error.log')
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
          <option value="/var/log/php-fpm/error.log">php-fpm/error.log</option>
          <option value="/var/log/php-fpm/www-error.log">www-error.log</option>
          <option value="/var/log/php8.2-fpm.log">php-fpm.log</option>
        </select>
        <button className="svc-cfg-btn" onClick={loadLog} disabled={loading}>
          {loading ? 'Loading...' : 'Refresh'}
        </button>
      </div>
      <pre className="svc-log-content">{logContent}</pre>
    </div>
  )
}
