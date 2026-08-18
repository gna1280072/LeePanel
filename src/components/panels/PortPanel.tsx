import { useState, useEffect, useCallback } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { useTranslation } from 'react-i18next'

interface PortInfo {
  port: number
  protocol: string
  address: string
  pid: number | null
  process: string | null
  user: string | null
}

interface PortPanelProps {
  sessionId: string | null
}

const DANGEROUS_PROCESSES = ['sshd', 'systemd', 'nginx', 'mysqld', 'docker', 'redis-server', 'php-fpm']

export default function PortPanel({ sessionId }: PortPanelProps) {
  const { t } = useTranslation()
  const [ports, setPorts] = useState<PortInfo[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState('')
  const [queryPort, setQueryPort] = useState('')
  const [mode, setMode] = useState<'all' | 'query'>('all')
  const [notice, setNotice] = useState<{ type: 'success' | 'info', text: string } | null>(null)
  const [confirmKill, setConfirmKill] = useState<PortInfo | null>(null)
  const [killConfirmInput, setKillConfirmInput] = useState('')
  const [killBusy, setKillBusy] = useState(false)

  const flashNotice = (type: 'success' | 'info', text: string) => {
    setNotice({ type, text })
    setTimeout(() => setNotice(null), 3000)
  }

  const fetchPorts = useCallback(async () => {
    if (!sessionId) return
    setLoading(true)
    setError('')
    try {
      const result = await invoke<PortInfo[]>('port_list', { sessionId })
      setPorts(result)
    } catch (e) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }, [sessionId])

  useEffect(() => {
    fetchPorts()
  }, [fetchPorts])

  const handleQuery = async () => {
    const p = parseInt(queryPort.trim(), 10)
    if (!sessionId || isNaN(p) || p < 1 || p > 65535) return
    setLoading(true)
    setError('')
    try {
      const result = await invoke<PortInfo[]>('port_query', { sessionId, port: p })
      setPorts(result)
      setMode('query')
    } catch (e) {
      setError(String(e))
    } finally {
      setLoading(false)
    }
  }

  const handleShowAll = () => {
    setQueryPort('')
    setMode('all')
    fetchPorts()
  }

  const handleKill = async (force: boolean) => {
    if (!sessionId || !confirmKill || confirmKill.pid == null) return
    if (killConfirmInput.trim() !== String(confirmKill.port)) return
    setKillBusy(true)
    setError('')
    try {
      await invoke('port_kill', {
        sessionId,
        pid: confirmKill.pid,
        force,
      })
      const killed = confirmKill
      setConfirmKill(null)
      setKillConfirmInput('')
      setKillBusy(false)
      flashNotice('success', t('port.killed', { pid: killed.pid, name: killed.process || '' }))
      if (mode === 'all') fetchPorts()
      else handleQuery()
    } catch (e) {
      setError(String(e))
      setKillBusy(false)
    }
  }

  const openKillDialog = (p: PortInfo) => {
    setKillConfirmInput('')
    setConfirmKill(p)
  }

  const isDangerous = (p: PortInfo) =>
    p.process != null && DANGEROUS_PROCESSES.some(d => p.process!.toLowerCase().includes(d))

  if (!sessionId) return <div className="sp-empty">{t('common.connectFirst')}</div>

  return (
    <div className="firewall-panel">
      <div className="firewall-header">
        <h2>{t('port.title')}</h2>
        <div className="firewall-header-actions">
          <button className="firewall-refresh" onClick={fetchPorts} disabled={loading}>
            {loading ? '...' : `↻ ${t('common.refresh')}`}
          </button>
        </div>
      </div>

      {error && <div className="firewall-error">{error}</div>}

      {notice && (
        <div className={`firewall-notice ${notice.type}`}>
          <span>{notice.type === 'success' ? '✅ ' : 'ℹ️ '}{notice.text}</span>
          <button className="firewall-notice-close" onClick={() => setNotice(null)}>✕</button>
        </div>
      )}

      {/* Query bar */}
      <div className="fw-toolbar" style={{ marginBottom: 12 }}>
        <input
          className="fw-search"
          placeholder={t('port.searchPlaceholder')}
          value={queryPort}
          inputMode="numeric"
          onChange={(e) => setQueryPort(e.target.value.replace(/[^\d]/g, ''))}
          onKeyDown={(e) => { if (e.key === 'Enter') handleQuery() }}
          style={{ maxWidth: 220 }}
        />
        <button
          className="firewall-add-btn"
          onClick={handleQuery}
          disabled={loading || !queryPort.trim()}
        >
          {t('port.query')}
        </button>
        {mode === 'query' && (
          <button className="firewall-refresh" onClick={handleShowAll}>
            {t('port.showAll')}
          </button>
        )}
        <div className="fw-toolbar-spacer" />
        <span className="fw-section-hint">
          {mode === 'all'
            ? t('port.listHint', { count: ports.length })
            : ports.length > 0
              ? t('port.occupied', { port: queryPort })
              : t('port.free', { port: queryPort })}
        </span>
      </div>

      {loading && !ports.length && <div className="sp-loading">{t('port.loading')}</div>}

      {!loading && ports.length === 0 && (
        <div className="firewall-empty">
          {mode === 'query' ? t('port.free', { port: queryPort }) : t('port.noListeners')}
        </div>
      )}

      {ports.length > 0 && (
        <div className="firewall-rules-table">
          <div className="firewall-table-header">
            <span className="fw-col-action">{t('port.pid')}</span>
            <span className="fw-col-proto">{t('port.protocol')}</span>
            <span className="fw-col-source">{t('port.address')}</span>
            <span className="fw-col-port">{t('port.port')}</span>
            <span className="fw-col-port">{t('port.process')}</span>
            <span className="fw-col-proto">{t('port.user')}</span>
            <span className="fw-col-ops"></span>
          </div>
          {ports.map((p, idx) => (
            <div
              key={`${p.port}-${p.protocol}-${p.pid}-${idx}`}
              className={`firewall-table-row ${isDangerous(p) ? 'danger' : ''}`}
            >
              <span className="fw-col-action">{p.pid ?? '—'}</span>
              <span className="fw-col-proto">{p.protocol.toUpperCase()}</span>
              <span className="fw-col-source fw-col-source-text" title={p.address}>{p.address}</span>
              <span className="fw-col-port" style={{ fontWeight: 600 }}>{p.port}</span>
              <span className="fw-col-port" title={p.process ?? ''}>{p.process ?? '—'}</span>
              <span className="fw-col-proto">{p.user ?? '—'}</span>
              <span className="fw-col-ops">
                <button
                  className="fw-delete-btn"
                  disabled={p.pid == null || killBusy}
                  title={p.pid == null ? t('port.noPidTitle') : t('port.killTitle', { pid: p.pid })}
                  onClick={() => openKillDialog(p)}
                >
                  ✕
                </button>
              </span>
            </div>
          ))}
        </div>
      )}

      {/* Confirm Kill Dialog */}
      {confirmKill && (
        <div className="firewall-confirm-overlay" onClick={() => !killBusy && setConfirmKill(null)}>
          <div className="firewall-confirm-dialog port-kill-dialog" onClick={(e) => e.stopPropagation()}>
            <div className="firewall-confirm-title">⚠️ {t('port.killTitle2', { pid: confirmKill.pid ?? '' })}</div>
            <div className="firewall-confirm-msg">
              <div style={{ fontSize: 16 }}>
                <b style={{ fontSize: 17 }}>{confirmKill.process || '—'}</b> (PID {confirmKill.pid}) —{' '}
                {t('port.protoPort', { protocol: confirmKill.protocol.toUpperCase(), port: confirmKill.port })}
              </div>

              {isDangerous(confirmKill) && (
                <div className="port-kill-danger">
                  {t('port.dangerHint')}
                </div>
              )}

              {/* Method explainer */}
              <div className="port-kill-methods">
                <div className="port-kill-method term">
                  <b>{t('port.sigterm')}</b>
                  <span>{t('port.sigtermDesc')}</span>
                </div>
                <div className="port-kill-method kill">
                  <b>{t('port.sigkill')}</b>
                  <span>{t('port.sigkillDesc')}</span>
                </div>
              </div>

              <div style={{ marginTop: 14 }}>
                <input
                  className="docker-confirm-input"
                  type="text"
                  inputMode="numeric"
                  placeholder={t('port.killConfirmPlaceholder', { port: confirmKill.port })}
                  value={killConfirmInput}
                  autoFocus
                  onChange={(e) => setKillConfirmInput(e.target.value.replace(/[^\d]/g, ''))}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter' && killConfirmInput.trim() === String(confirmKill.port)) handleKill(false)
                  }}
                  style={{ width: '100%', boxSizing: 'border-box' }}
                />
                <div className="port-kill-confirm-hint">
                  {t('port.killConfirmHint', { port: confirmKill.port })}
                </div>
              </div>
            </div>
            <div className="firewall-confirm-actions" style={{ marginTop: 18 }}>
              <button
                className="firewall-confirm-btn cancel"
                onClick={() => { setConfirmKill(null); setKillConfirmInput('') }}
                disabled={killBusy}
              >
                {t('common.cancel')}
              </button>
              <button
                className="firewall-confirm-btn"
                style={{ borderColor: 'var(--warn, #d98e00)', color: 'var(--warn, #b8860b)' }}
                onClick={() => handleKill(false)}
                disabled={killBusy || killConfirmInput.trim() !== String(confirmKill.port)}
              >
                {killBusy ? '...' : t('port.sigterm')}
              </button>
              <button
                className="firewall-confirm-btn danger"
                onClick={() => handleKill(true)}
                disabled={killBusy || killConfirmInput.trim() !== String(confirmKill.port)}
              >
                {killBusy ? '...' : t('port.sigkill')}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}
